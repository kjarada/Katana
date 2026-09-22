#include "render_view_widget.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>

#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QWheelEvent>

namespace katana::qt {

namespace {

// Radians per pixel of drag. A full turn in roughly 800 pixels, which is about
// a screen width and matches what every other CAD package feels like.
constexpr double kOrbitPerPixel = 0.008;
// One wheel notch is 120 eighths of a degree; 1.15 per notch gives a
// comfortable 2x in five notches.
constexpr double kZoomPerNotch = 1.15;

} // namespace

RenderViewWidget::RenderViewWidget(ViewContext context, katana::cad::ViewportCell* cell,
                                   QWidget* parent)
    : QWidget(parent), context_(context), cell_(cell)
{
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    // The whole widget is painted from the framebuffer every time, so Qt need
    // not clear it first: that is a full-window fill saved per frame.
    setAttribute(Qt::WA_OpaquePaintEvent, true);
    setMinimumSize(40, 40);
}

void RenderViewWidget::setContext(const ViewContext& context)
{
    context_ = context;
    invalidateScene();
}

void RenderViewWidget::setCell(katana::cad::ViewportCell* cell)
{
    cell_ = cell;
    framed_ = false;
    invalidateScene();
}

katana::render::Camera& RenderViewWidget::camera()
{
    static katana::render::Camera fallback; // only reached with no cell attached
    return cell_ != nullptr ? cell_->camera : fallback;
}

void RenderViewWidget::invalidateScene()
{
    sceneDirty_ = true;
    update();
}

void RenderViewWidget::setStandardView(katana::render::StandardView view)
{
    camera().setStandardView(view);
    update();
}

void RenderViewWidget::setProjection(katana::render::Projection projection)
{
    camera().setProjection(projection);
    update();
}

void RenderViewWidget::setVerticalExaggeration(double factor)
{
    if (!std::isfinite(factor) || factor <= 0.0) {
        return;
    }
    context_.options.verticalExaggeration = factor;
    // The datum is the middle of the surfaces so exaggerating does not also
    // launch the model off the top of the screen.
    if (context_.document != nullptr && context_.surfaces != nullptr) {
        katana::cad::SceneOptions flat = context_.options;
        flat.verticalExaggeration = 1.0;
        const auto box =
            katana::cad::sceneBounds(*context_.document, *context_.surfaces, flat, meshes());
        context_.options.exaggerationDatum = box.empty() ? 0.0 : box.center().z;
    }
    invalidateScene();
    zoomExtents();
}

void RenderViewWidget::rebuildIfNeeded()
{
    if (!sceneDirty_ || context_.document == nullptr) {
        return;
    }
    static const std::vector<katana::cad::SceneSurface> kNoSurfaces;
    const auto& surfaces = context_.surfaces != nullptr ? *context_.surfaces : kNoSurfaces;
    builder_.build(*context_.document, surfaces, context_.options, list_, meshes());
    sceneDirty_ = false;
}

void RenderViewWidget::zoomExtents()
{
    if (context_.document == nullptr) {
        return;
    }
    static const std::vector<katana::cad::SceneSurface> kNoSurfaces;
    const auto& surfaces = context_.surfaces != nullptr ? *context_.surfaces : kNoSurfaces;
    auto box = katana::cad::sceneBounds(*context_.document, surfaces, context_.options, meshes());
    if (box.empty()) {
        // Nothing to frame: show a sensible patch of ground rather than
        // leaving the camera wherever it was, which looks like a broken view.
        box = katana::math::AABB(katana::math::Vec3(-50.0, -50.0, -5.0),
                                 katana::math::Vec3(50.0, 50.0, 5.0));
    }
    camera().frame(box);
    framed_ = true;
    update();
}

void RenderViewWidget::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    const int w = std::max(width(), 1);
    const int h = std::max(height(), 1);
    if (auto status = framebuffer_.resize(w, h); !status) {
        if (onStatus) {
            onStatus(QString::fromStdString(status.error().describe()));
        }
        return;
    }
    camera().setViewportSize(w, h);
    if (cell_ != nullptr) {
        cell_->pixelWidth = w;
        cell_->pixelHeight = h;
    }
}

void RenderViewWidget::paintEvent(QPaintEvent* /*event*/)
{
    QPainter painter(this);
    if (framebuffer_.empty()) {
        painter.fillRect(rect(), QColor(30, 30, 34));
        return;
    }
    if (!framed_) {
        zoomExtents();
    }
    rebuildIfNeeded();

    const auto started = std::chrono::steady_clock::now();
    katana::render::RenderOptions options;
    options.background = katana::render::rgba(28, 30, 36);
    const auto result = rasterizer_.render(list_, camera(), framebuffer_, options);
    const auto finished = std::chrono::steady_clock::now();
    lastFrameMs_ =
        std::chrono::duration<double, std::milli>(finished - started).count();

    if (!result) {
        painter.fillRect(rect(), QColor(60, 20, 20));
        painter.setPen(Qt::white);
        painter.drawText(rect(), Qt::AlignCenter,
                         QString::fromStdString(result.error().describe()));
        return;
    }
    stats_ = *result;

    // No copy: the framebuffer's bytes ARE the image's bytes for this call.
    const QImage image(reinterpret_cast<const uchar*>(framebuffer_.color().data()),
                       framebuffer_.width(), framebuffer_.height(),
                       framebuffer_.width() * static_cast<int>(sizeof(katana::render::Rgba)),
                       QImage::Format_ARGB32);
    painter.drawImage(0, 0, image);

    if (onStatus) {
        onStatus(QString("%1  %2 tri  %3 ms")
                     .arg(QString::fromLatin1(katana::cad::toString(
                         cell_ != nullptr ? cell_->kind : katana::cad::ViewKind::Model3D)))
                     .arg(stats_.trianglesRasterised)
                     .arg(lastFrameMs_, 0, 'f', 1));
    }
}

void RenderViewWidget::mousePressEvent(QMouseEvent* event)
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
        // An orthographic elevation view is a measured drawing; orbiting it
        // would quietly turn it into something you cannot scale off. Pan only.
        drag_ = (cell_ != nullptr && cell_->kind == katana::cad::ViewKind::Elevation)
                    ? Drag::Pan
                    : Drag::Orbit;
    } else {
        drag_ = Drag::None;
    }
}

void RenderViewWidget::mouseMoveEvent(QMouseEvent* event)
{
    if (drag_ == Drag::None) {
        return;
    }
    const QPoint delta = event->pos() - lastMouse_;
    lastMouse_ = event->pos();
    if (drag_ == Drag::Orbit) {
        // Dragging right turns the model to the right, so the camera goes the
        // other way; dragging down tips the top towards you.
        camera().orbit(-delta.x() * kOrbitPerPixel, delta.y() * kOrbitPerPixel);
    } else {
        camera().panPixels(delta.x(), delta.y());
    }
    update();
}

void RenderViewWidget::mouseReleaseEvent(QMouseEvent* /*event*/) { drag_ = Drag::None; }

void RenderViewWidget::mouseDoubleClickEvent(QMouseEvent* /*event*/) { zoomExtents(); }

void RenderViewWidget::wheelEvent(QWheelEvent* event)
{
    const double notches = event->angleDelta().y() / 120.0;
    if (notches == 0.0) {
        return;
    }
    const double factor = std::pow(1.0 / kZoomPerNotch, notches);
    const QPointF position = event->position();
    camera().dollyAtPixel(factor, position.x(), position.y());
    update();
    event->accept();
}

void RenderViewWidget::keyPressEvent(QKeyEvent* event)
{
    using katana::render::StandardView;
    switch (event->key()) {
    case Qt::Key_1:
        setStandardView(StandardView::Top);
        return;
    case Qt::Key_2:
        setStandardView(StandardView::Front);
        return;
    case Qt::Key_3:
        setStandardView(StandardView::Right);
        return;
    case Qt::Key_4:
        setStandardView(StandardView::Back);
        return;
    case Qt::Key_5:
        setStandardView(StandardView::Left);
        return;
    case Qt::Key_0:
        setStandardView(StandardView::IsoSouthWest);
        return;
    case Qt::Key_P:
        setProjection(camera().projection() == katana::render::Projection::Perspective
                          ? katana::render::Projection::Orthographic
                          : katana::render::Projection::Perspective);
        return;
    case Qt::Key_E:
        zoomExtents();
        return;
    default:
        break;
    }
    QWidget::keyPressEvent(event);
}

} // namespace katana::qt
