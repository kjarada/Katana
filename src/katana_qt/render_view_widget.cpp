#include "render_view_widget.hpp"

#include "theme.hpp"
#include "view_focus.hpp"

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

// The ground behind the model, and behind the empty view's message.
const QColor kBackground(28, 30, 36);

} // namespace

RenderViewWidget::RenderViewWidget(ViewContext context, katana::cad::ViewState& state,
                                   QWidget* parent)
    : QWidget(parent), context_(context), state_(state)
{
    context_.options.layers = &state_.layers;
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    // The whole widget is painted from the framebuffer every time, so Qt need
    // not clear it first: that is a full-window fill saved per frame.
    setAttribute(Qt::WA_OpaquePaintEvent, true);
    setMinimumSize(40, 40);
    listenTo(context_.document);
    activateOnFocus(*this, [this] {
        if (onActivated) {
            onActivated();
        }
    });
}

void RenderViewWidget::setContext(const ViewContext& context)
{
    context_ = context;
    context_.options.layers = &state_.layers;
    listenTo(context_.document);
    invalidateScene();
}

void RenderViewWidget::listenTo(katana::cad::Document* document)
{
    if (document == listenedDocument_) {
        return;
    }
    documentListener_.reset();
    listenedDocument_ = document;
    if (document != nullptr) {
        // Every edit, undo, layer change and selection change: the 3D view
        // was rebuilt only when something else happened to invalidate it, so
        // a line drawn in plan never appeared here and an erased one stayed
        // (audit QT-05). The rebuild is lazy - this marks the scene dirty and
        // asks for one paint, and Qt folds a burst of changes into that paint.
        documentListener_ = document->addListener([this] { invalidateScene(); });
    }
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
    // The datum is the middle of what is drawn so exaggerating does not also
    // launch the model off the top of the screen. Not only when there are
    // surfaces: a scene of meshes alone needs its datum as much.
    if (context_.document != nullptr) {
        katana::cad::SceneOptions flat = context_.options;
        flat.verticalExaggeration = 1.0;
        const auto box = katana::cad::sceneBounds(*context_.document, surfaces(), flat, meshes());
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
    builder_.build(*context_.document, surfaces(), context_.options, list_, meshes());
    sceneDirty_ = false;
    // Empty when the list holds the grid and nothing else. Counted against a
    // grid built alone, by the same builder, rather than by asking
    // cad::sceneBounds: that walks every entity again, and this now runs on
    // every edit - measured (Debug, 100 000 lines) at 21 ms on top of the
    // 90 ms build. A grid's line count does not depend on where it is centred.
    gridOnly_.clear();
    builder_.appendGrid(context_.options, katana::math::AABB{}, gridOnly_);
    sceneEmpty_ = list_.triangles.empty() && list_.points.empty() &&
                  list_.lines.size() <= gridOnly_.lines.size();
    if (framedEmpty_ && !sceneEmpty_) {
        zoomExtents();
    }
}

void RenderViewWidget::zoomExtents()
{
    if (context_.document == nullptr) {
        return;
    }
    auto box = katana::cad::sceneBounds(*context_.document, surfaces(), context_.options, meshes());
    framedEmpty_ = box.empty();
    if (framedEmpty_) {
        // Nothing to frame: show a patch of ground round the origin rather
        // than leaving the camera wherever it was, which looks like a broken
        // view - and the paint says there is nothing to show. Five grid cells
        // each way (SceneOptions::gridSpacing, 10 m by default, so 100 m
        // across) and a tenth of that in height, so the frame is the ground
        // plane seen at a slant rather than a cube.
        const double spacing =
            std::isfinite(context_.options.gridSpacing) && context_.options.gridSpacing > 0.0
                ? context_.options.gridSpacing
                : 10.0;
        const double half = 5.0 * spacing;
        box = katana::math::AABB(katana::math::Vec3(-half, -half, -0.1 * half),
                                 katana::math::Vec3(half, half, 0.1 * half));
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
}

void RenderViewWidget::paintEvent(QPaintEvent* /*event*/)
{
    QPainter painter(this);
    if (framebuffer_.empty()) {
        painter.fillRect(rect(), kBackground);
        return;
    }
    if (!framed_) {
        zoomExtents();
    }
    rebuildIfNeeded();

    const auto started = std::chrono::steady_clock::now();
    katana::render::RenderOptions options;
    options.background =
        katana::render::rgba(kBackground.red(), kBackground.green(), kBackground.blue());
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
    if (sceneEmpty_) {
        drawEmptyMessage(painter);
    }

    if (onFrameStats) {
        onFrameStats(QString("%1  %2 tri  %3 ms")
                         .arg(QString::fromLatin1(katana::cad::toString(state_.kind)))
                         .arg(stats_.trianglesRasterised)
                         .arg(lastFrameMs_, 0, 'f', 1));
    }
}

// An empty 3D view looked broken: a grid and nothing on it, with no word of
// why. It says what would put something there.
void RenderViewWidget::drawEmptyMessage(QPainter& painter) const
{
    const QString message = QStringLiteral("Nothing to show in 3D yet.\n"
                                           "Draw or import something, or build a surface "
                                           "from the Terrain menu.");
    const QRect area = rect().adjusted(12, 12, -12, -12);
    const int flags = Qt::AlignCenter | Qt::TextWordWrap;
    const QRect text = painter.fontMetrics().boundingRect(area, flags, message);
    if (text.height() > area.height()) {
        return; // too small a view to say it in; the grid alone says less wrongly
    }
    painter.save();
    // A backing of the view's own ground so the grid does not strike through
    // the words, and the chrome's muted text so it reads as the application
    // talking rather than as something in the model.
    painter.setPen(Qt::NoPen);
    QColor backing = kBackground;
    backing.setAlpha(225);
    painter.setBrush(backing);
    painter.drawRoundedRect(text.adjusted(-12, -8, 12, 8), 6, 6);
    painter.setPen(theme::textMuted());
    painter.drawText(area, flags, message);
    painter.restore();
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
        drag_ = state_.kind == katana::cad::ViewKind::Elevation ? Drag::Pan : Drag::Orbit;
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
