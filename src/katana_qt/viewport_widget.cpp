#include "viewport_widget.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

#include <QFont>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPolygonF>
#include <QWheelEvent>

#include "katana/cad/selection.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/interop/reference_data.hpp"

namespace katana::qt {

namespace cad = katana::cad;
namespace cmd = katana::commands;
using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

namespace {

const QColor kBackground(0x1e, 0x23, 0x29);
const QColor kGridMinor(0x2a, 0x31, 0x39);
const QColor kGridMajor(0x38, 0x42, 0x4d);
const QColor kAxis(0x5a, 0x68, 0x75);
const QColor kSelection(0xff, 0x9f, 0x1c);
const QColor kPreview(0x4c, 0xc9, 0xf0);
const QColor kSnapMarker(0xf7, 0xd0, 0x3c);

constexpr double kPickAperturePixels = 8.0;
constexpr double kSnapAperturePixels = 12.0;
constexpr double kPointMarkerPixels = 4.0;
constexpr double kWheelZoomStep = 1.2;
// Drags shorter than this are clicks, not selection boxes.
constexpr double kDragThresholdPixels = 4.0;

QColor toQColor(const katana::entity::Color& color)
{
    return QColor(color.r, color.g, color.b, color.a);
}

} // namespace

const char* toString(Tool tool)
{
    switch (tool) {
    case Tool::Select:
        return "Select";
    case Tool::Point:
        return "Point";
    case Tool::Line:
        return "Line";
    case Tool::Polyline:
        return "Polyline";
    case Tool::Rectangle:
        return "Rectangle";
    case Tool::Circle:
        return "Circle";
    case Tool::Arc:
        return "Arc";
    case Tool::Move:
        return "Move";
    case Tool::Copy:
        return "Copy";
    }
    return "Unknown";
}

ViewportWidget::ViewportWidget(cad::Document& document, QWidget* parent)
    : QWidget(parent), document_(document)
{
    setMinimumSize(480, 320);
    setMouseTracking(true);
    setFocusPolicy(Qt::StrongFocus);
    setCursor(Qt::CrossCursor);
    view_.scale = 10.0;
    document_.addListener([this] { update(); });
}

QPointF ViewportWidget::toScreen(const Point2& world) const
{
    const Point2 p = view_.worldToScreen(world);
    return QPointF(p.x, p.y);
}

ViewportWidget::Point2 ViewportWidget::toWorld(const QPointF& screen) const
{
    return view_.screenToWorld(Point2(screen.x(), screen.y()));
}

// ---- public controls --------------------------------------------------------------------

void ViewportWidget::setTool(Tool tool)
{
    points_.clear();
    boxStart_.reset();
    tool_ = tool;
    updatePrompt();
    if (onToolChanged) {
        onToolChanged(tool_);
    }
    update();
}

void ViewportWidget::zoomExtents()
{
    view_.resize(width(), height());
    // Zoom Extents means everything the user can see, so imported imagery and
    // point clouds count. A drawing that is empty except for an orthophoto
    // would otherwise fit an empty box and leave the photo off screen.
    Box2 bounds = document_.model().entities.bounds();
    if (reference_ != nullptr) {
        bounds.expand(reference_->visibleBounds());
    }
    view_.fit(bounds, 0.08);
    update();
}

void ViewportWidget::zoomTo(const Box2& bounds)
{
    if (bounds.empty()) {
        return;
    }
    view_.resize(width(), height());
    view_.fit(bounds, 0.08);
    update();
}

void ViewportWidget::setReferenceData(katana::interop::ReferenceData* reference)
{
    // Not named `data`: QWidget already has a member of that name, and -Wshadow
    // is an error here.
    reference_ = reference;
    invalidateReferenceCache();
}

void ViewportWidget::invalidateReferenceCache()
{
    rasterCache_.clear();
    cloudCache_.clear();
    update();
}

void ViewportWidget::setGridVisible(bool visible)
{
    gridVisible_ = visible;
    update();
}

void ViewportWidget::setSnapEnabled(bool enabled)
{
    snapEnabled_ = enabled;
    activeSnap_.reset();
    update();
}

void ViewportWidget::setSnapModes(cad::SnapModes modes)
{
    snapModes_ = modes;
}

void ViewportWidget::cancel()
{
    if (!points_.empty() || boxStart_) {
        points_.clear();
        boxStart_.reset();
    } else if (tool_ != Tool::Select) {
        setTool(Tool::Select);
        return;
    } else if (!document_.selection().empty()) {
        document_.selection().clear();
        document_.notifySelectionChanged();
    }
    updatePrompt();
    update();
}

void ViewportWidget::resetInteraction()
{
    points_.clear();
    boxStart_.reset();
    activeSnap_.reset();
    updatePrompt();
    update();
}

void ViewportWidget::updatePrompt()
{
    if (!onPrompt) {
        return;
    }
    const std::size_t n = points_.size();
    QString text;
    switch (tool_) {
    case Tool::Select:
        text = "Select: click an entity, drag right for a window, drag left for crossing";
        break;
    case Tool::Point:
        text = "Point: pick a position";
        break;
    case Tool::Line:
        text = n == 0 ? "Line: pick the start point" : "Line: pick the next point (Enter to finish)";
        break;
    case Tool::Polyline:
        text = n == 0 ? "Polyline: pick the first vertex"
                      : "Polyline: pick the next vertex (Enter to finish, C to close)";
        break;
    case Tool::Rectangle:
        text = n == 0 ? "Rectangle: pick the first corner" : "Rectangle: pick the opposite corner";
        break;
    case Tool::Circle:
        text = n == 0 ? "Circle: pick the centre" : "Circle: pick a point on the circumference";
        break;
    case Tool::Arc:
        text = n == 0   ? "Arc: pick the start point"
               : n == 1 ? "Arc: pick a point on the arc"
                        : "Arc: pick the end point";
        break;
    case Tool::Move:
    case Tool::Copy:
        text = QString(toString(tool_)) +
               (n == 0 ? ": pick the base point" : ": pick the destination");
        break;
    }
    onPrompt(text);
}

void ViewportWidget::run(cmd::CommandPtr command)
{
    const auto status = document_.execute(std::move(command));
    if (!status && onError) {
        onError(QString::fromStdString(status.error().describe()));
    }
}

// ---- interaction ------------------------------------------------------------------------

void ViewportWidget::updateCursor(const QPointF& screen)
{
    const Point2 raw = toWorld(screen);
    cursorWorld_ = raw;
    activeSnap_.reset();

    if (snapEnabled_ && tool_ != Tool::Select) {
        cad::SnapRequest request;
        request.cursor = raw;
        request.aperture = view_.pixelsToWorld(kSnapAperturePixels);
        request.modes = snapModes_;
        if (!points_.empty()) {
            request.from = points_.back();
        }
        request.gridSpacing = gridVisible_ ? cad::gridSpacing(view_.scale) : 0.0;
        activeSnap_ = cad::snap(document_.model(), request);
        if (activeSnap_) {
            cursorWorld_ = activeSnap_->point;
        }
    }
    if (onCursorMoved) {
        onCursorMoved(cursorWorld_, activeSnap_);
    }
}

void ViewportWidget::acceptPoint(const Point2& point)
{
    const cmd::EntityAttributes attributes = document_.currentAttributes();
    switch (tool_) {
    case Tool::Select:
        return;
    case Tool::Point:
        run(cmd::createPoint(point, attributes));
        break;
    case Tool::Line:
        if (!points_.empty()) {
            run(cmd::createLine(points_.back(), point, attributes));
        }
        points_ = {point}; // chain: the end becomes the next start
        break;
    case Tool::Polyline:
        points_.push_back(point);
        break;
    case Tool::Rectangle:
        if (points_.empty()) {
            points_.push_back(point);
        } else {
            const auto rectangle = katana::geometry::Rectangle2::fromCorners(points_[0], point);
            run(cmd::createPolyline(rectangle.toPolyline(), attributes));
            points_.clear();
        }
        break;
    case Tool::Circle:
        if (points_.empty()) {
            points_.push_back(point);
        } else {
            run(cmd::createCircle(points_[0], points_[0].distanceTo(point), attributes));
            points_.clear();
        }
        break;
    case Tool::Arc:
        points_.push_back(point);
        if (points_.size() == 3) {
            const auto arc = Arc2::throughPoints(points_[0], points_[1], points_[2]);
            if (arc) {
                run(cmd::createArc(*arc, attributes));
            } else if (onError) {
                onError("The three points are collinear or coincident.");
            }
            points_.clear();
        }
        break;
    case Tool::Move:
    case Tool::Copy:
        if (document_.selection().empty()) {
            if (onError) {
                onError("Select the entities first.");
            }
            setTool(Tool::Select);
            return;
        }
        if (points_.empty()) {
            points_.push_back(point);
        } else {
            const Vec2 delta = point - points_[0];
            const auto ids = document_.selection().ids();
            run(tool_ == Tool::Move ? cmd::moveEntities(ids, delta) : cmd::copyEntities(ids, delta));
            points_.clear();
        }
        break;
    }
    updatePrompt();
    update();
}

void ViewportWidget::finishOperation(bool close)
{
    if (tool_ == Tool::Polyline && points_.size() >= 2) {
        run(cmd::createPolyline(Polyline2{points_, close && points_.size() >= 3},
                                document_.currentAttributes()));
    }
    points_.clear();
    updatePrompt();
    update();
}

void ViewportWidget::selectAt(const QPointF& screen, Qt::KeyboardModifiers modifiers)
{
    const auto picked = cad::pickEntity(document_.model(), toWorld(screen),
                                        view_.pixelsToWorld(kPickAperturePixels));
    cad::SelectionSet& selection = document_.selection();
    if (modifiers & Qt::ControlModifier) {
        if (picked) {
            selection.toggle(*picked);
        }
    } else if (modifiers & Qt::ShiftModifier) {
        if (picked) {
            selection.add(*picked);
        }
    } else {
        selection.clear();
        if (picked) {
            selection.add(*picked);
        }
    }
    document_.notifySelectionChanged();
}

void ViewportWidget::selectInBox(const QPointF& from, const QPointF& to,
                                 Qt::KeyboardModifiers modifiers)
{
    Box2 box;
    box.expand(toWorld(from));
    box.expand(toWorld(to));
    // Dragging left to right selects what is fully inside; right to left also
    // takes whatever the box touches.
    const auto mode = to.x() >= from.x() ? cad::BoxSelectionMode::Window
                                         : cad::BoxSelectionMode::Crossing;
    const auto picked = cad::pickInBox(document_.model(), box, mode);
    cad::SelectionSet& selection = document_.selection();
    if (!(modifiers & (Qt::ShiftModifier | Qt::ControlModifier))) {
        selection.clear();
    }
    for (const auto id : picked) {
        selection.add(id);
    }
    document_.notifySelectionChanged();
}

void ViewportWidget::mousePressEvent(QMouseEvent* event)
{
    setFocus();
    lastMouse_ = event->position();
    if (event->button() == Qt::MiddleButton) {
        panning_ = true;
        setCursor(Qt::ClosedHandCursor);
        return;
    }
    if (event->button() == Qt::RightButton) {
        if (points_.empty()) {
            cancel();
        } else {
            finishOperation(false);
        }
        return;
    }
    if (event->button() != Qt::LeftButton) {
        return;
    }
    if (tool_ == Tool::Select) {
        boxStart_ = event->position();
        boxEnd_ = event->position();
        return;
    }
    updateCursor(event->position());
    acceptPoint(cursorWorld_);
}

void ViewportWidget::mouseMoveEvent(QMouseEvent* event)
{
    if (panning_) {
        const QPointF delta = event->position() - lastMouse_;
        view_.panByPixels(delta.x(), delta.y());
    } else if (boxStart_) {
        boxEnd_ = event->position();
    }
    lastMouse_ = event->position();
    updateCursor(event->position());
    update();
}

void ViewportWidget::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() == Qt::MiddleButton) {
        panning_ = false;
        setCursor(Qt::CrossCursor);
        return;
    }
    if (event->button() == Qt::LeftButton && boxStart_) {
        const QPointF from = *boxStart_;
        const QPointF to = event->position();
        boxStart_.reset();
        const bool dragged = std::abs(to.x() - from.x()) > kDragThresholdPixels ||
                             std::abs(to.y() - from.y()) > kDragThresholdPixels;
        if (dragged) {
            selectInBox(from, to, event->modifiers());
        } else {
            selectAt(to, event->modifiers());
        }
        update();
    }
}

void ViewportWidget::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() == Qt::MiddleButton) {
        zoomExtents();
        return;
    }
    // Qt delivers a double click as press, release, DOUBLECLICK, release - the
    // SECOND press arrives here and never reaches mousePressEvent. Swallowing it
    // means a user clicking quickly while drawing silently loses that vertex, so
    // a drawing tool has to consume it exactly as it would an ordinary press.
    // Select is left alone: there a double click is not a second pick.
    if (tool_ != Tool::Select) {
        mousePressEvent(event);
    }
}

void ViewportWidget::wheelEvent(QWheelEvent* event)
{
    const double notches = event->angleDelta().y() / 120.0;
    if (notches != 0.0) {
        view_.zoomAt(Point2(event->position().x(), event->position().y()),
                     std::pow(kWheelZoomStep, notches));
        updateCursor(event->position());
        update();
    }
    event->accept();
}

void ViewportWidget::keyPressEvent(QKeyEvent* event)
{
    switch (event->key()) {
    case Qt::Key_Escape:
        cancel();
        return;
    case Qt::Key_Return:
    case Qt::Key_Enter:
        finishOperation(false);
        return;
    case Qt::Key_C:
        if (tool_ == Tool::Polyline && points_.size() >= 3) {
            finishOperation(true);
            return;
        }
        break;
    case Qt::Key_Delete:
        if (!document_.selection().empty()) {
            run(cmd::deleteEntities(document_.selection().ids()));
        }
        return;
    default:
        break;
    }
    QWidget::keyPressEvent(event);
}

void ViewportWidget::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    view_.resize(width(), height());
    if (!viewInitialised_) {
        viewInitialised_ = true;
        if (!document_.model().entities.empty()) {
            zoomExtents();
        }
    }
}

// ---- painting ---------------------------------------------------------------------------

void ViewportWidget::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.fillRect(rect(), kBackground);
    view_.resize(width(), height());

    // Imagery sits beneath everything: it is a backdrop, and the grid has to
    // stay legible over it. Point clouds sit above the grid but below the
    // drawing, so drawn geometry is never obscured by survey returns.
    drawRasters(painter);
    if (gridVisible_) {
        drawGrid(painter);
    }
    drawPointClouds(painter);
    painter.setRenderHint(QPainter::Antialiasing, true);
    drawEntities(painter);
    drawPreview(painter);

    if (boxStart_) {
        const bool window = boxEnd_.x() >= boxStart_->x();
        QColor fill = window ? QColor(0x4c, 0xc9, 0xf0, 40) : QColor(0x7b, 0xd3, 0x89, 40);
        painter.setPen(QPen(fill.lighter(160), 1, window ? Qt::SolidLine : Qt::DashLine));
        painter.setBrush(fill);
        painter.drawRect(QRectF(*boxStart_, boxEnd_).normalized());
        painter.setBrush(Qt::NoBrush);
    }
    drawSnapMarker(painter);
}

void ViewportWidget::drawRasters(QPainter& painter) const
{
    if (reference_ == nullptr) {
        return;
    }
    for (const katana::interop::RasterOverlay& raster : reference_->rasters()) {
        if (!raster.visible || raster.width <= 0 || raster.height <= 0) {
            continue;
        }

        // Cache the QImage: rebuilding it from the RGBA bytes every frame would
        // copy tens of megabytes per repaint.
        const auto cached = std::find_if(
            rasterCache_.begin(), rasterCache_.end(),
            [&raster](const RasterCache& entry) { return entry.id == raster.id; });
        if (cached == rasterCache_.end()) {
            QImage image(reinterpret_cast<const uchar*>(raster.rgba.data()), raster.width,
                         raster.height, raster.width * 4, QImage::Format_RGBA8888);
            // copy(): the QImage above only borrows the vector's buffer, and the
            // cache must outlive this loop iteration.
            rasterCache_.push_back(RasterCache{raster.id, image.copy()});
            continue; // drawn on the next pass through, once the cache is warm
        }

        // Pixel -> world is the geotransform; world -> screen is the view. The
        // composition is itself affine, so it is handed to QPainter as one
        // transform rather than resampling the image here.
        //
        //   world.x = g0 + px*g1 + py*g2       screen.x = w/2 + (world.x - cx)*s
        //   world.y = g3 + px*g4 + py*g5       screen.y = h/2 - (world.y - cy)*s
        //
        // so screen.x = [w/2 + (g0-cx)s] + px*(g1 s) + py*(g2 s)
        //    screen.y = [h/2 - (g3-cy)s] + px*(-g4 s) + py*(-g5 s)
        const auto& g = raster.geotransform;
        const double s = view_.scale;
        const double dx = 0.5 * width() + (g[0] - view_.center.x) * s;
        const double dy = 0.5 * height() - (g[3] - view_.center.y) * s;
        const QTransform transform(g[1] * s, -g[4] * s, g[2] * s, -g[5] * s, dx, dy);

        painter.save();
        painter.setOpacity(std::clamp(raster.opacity, 0.0, 1.0));
        painter.setTransform(transform);
        // Smooth only when magnifying past 1:1; downsampling a huge image with
        // smoothing on is slow and makes little visible difference.
        painter.setRenderHint(QPainter::SmoothPixmapTransform,
                              std::abs(g[1] * s) > 1.0);
        painter.drawImage(QPointF(0.0, 0.0), cached->image);
        painter.restore();
    }
}

void ViewportWidget::drawPointClouds(QPainter& painter) const
{
    if (reference_ == nullptr || width() <= 0 || height() <= 0) {
        return;
    }

    for (const katana::interop::PointCloudLayer& cloud : reference_->pointClouds()) {
        if (!cloud.visible || cloud.points.empty()) {
            continue;
        }

        // Per-point colour depends only on the layer and its mode, so it is
        // computed once and cached; only the projection is redone per frame.
        auto cached = std::find_if(cloudCache_.begin(), cloudCache_.end(),
                                   [&cloud](const CloudCache& entry) {
                                       return entry.id == cloud.id &&
                                              entry.mode == cloud.colorMode;
                                   });
        if (cached == cloudCache_.end()) {
            // Drop any stale entry for this layer whose mode has changed.
            cloudCache_.erase(std::remove_if(cloudCache_.begin(), cloudCache_.end(),
                                             [&cloud](const CloudCache& entry) {
                                                 return entry.id == cloud.id;
                                             }),
                              cloudCache_.end());

            double minimum = 0.0;
            double maximum = 1.0;
            if (cloud.colorMode == katana::interop::PointColorMode::Intensity) {
                minimum = std::numeric_limits<double>::max();
                maximum = std::numeric_limits<double>::lowest();
                for (const auto& point : cloud.points) {
                    minimum = std::min(minimum, point.intensity);
                    maximum = std::max(maximum, point.intensity);
                }
            } else {
                minimum = cloud.bounds.minZ;
                maximum = cloud.bounds.maxZ;
            }

            CloudCache entry;
            entry.id = cloud.id;
            entry.mode = cloud.colorMode;
            entry.colors.reserve(cloud.points.size());
            for (const auto& point : cloud.points) {
                const katana::interop::Rgb rgb =
                    katana::interop::colorForPoint(point, cloud.colorMode, minimum, maximum);
                entry.colors.push_back(qRgb(rgb.r, rgb.g, rgb.b));
            }
            cloudCache_.push_back(std::move(entry));
            cached = std::prev(cloudCache_.end());
        }

        // Splat into an image rather than calling QPainter per point: a
        // QPainter::drawPoint costs microseconds, which at two million points is
        // seconds per frame. Writing pixels directly is a handful of
        // instructions each and keeps panning interactive.
        QImage layer(width(), height(), QImage::Format_ARGB32_Premultiplied);
        layer.fill(::Qt::transparent);
        auto* bits = reinterpret_cast<QRgb*>(layer.bits());
        const int stride = static_cast<int>(layer.bytesPerLine() / sizeof(QRgb));

        const double s = view_.scale;
        const double halfWidth = 0.5 * width();
        const double halfHeight = 0.5 * height();
        const int radius = std::max(0, static_cast<int>(cloud.pointSize) - 1);

        for (std::size_t i = 0; i < cloud.points.size(); ++i) {
            const auto& point = cloud.points[i];
            const double sx = halfWidth + (point.x - view_.center.x) * s;
            const double sy = halfHeight - (point.y - view_.center.y) * s;
            // Reject before the cast: converting a coordinate far outside int
            // range is undefined behaviour, and panning a UTM-scale cloud when
            // zoomed in produces exactly such values.
            if (!(sx > -1.0e6 && sx < 1.0e6 && sy > -1.0e6 && sy < 1.0e6)) {
                continue;
            }
            const int px = static_cast<int>(sx);
            const int py = static_cast<int>(sy);
            if (px < 0 || py < 0 || px >= width() || py >= height()) {
                continue;
            }
            const QRgb color = cached->colors[i] | 0xff000000u;
            if (radius == 0) {
                bits[py * stride + px] = color;
                continue;
            }
            for (int oy = -radius; oy <= radius; ++oy) {
                const int y = py + oy;
                if (y < 0 || y >= height()) {
                    continue;
                }
                for (int ox = -radius; ox <= radius; ++ox) {
                    const int x = px + ox;
                    if (x >= 0 && x < width()) {
                        bits[y * stride + x] = color;
                    }
                }
            }
        }
        painter.drawImage(0, 0, layer);
    }
}

void ViewportWidget::drawGrid(QPainter& painter) const
{
    const double spacing = cad::gridSpacing(view_.scale);
    const Box2 visible = view_.visibleWorldBounds();
    const auto firstIndex = [&](double lo) { return static_cast<long long>(std::floor(lo / spacing)); };
    const auto lastIndex = [&](double hi) { return static_cast<long long>(std::ceil(hi / spacing)); };

    // Every fifth line is a major line so that distances can be read off.
    for (long long i = firstIndex(visible.min.x); i <= lastIndex(visible.max.x); ++i) {
        painter.setPen(QPen(i % 5 == 0 ? kGridMajor : kGridMinor, 1));
        const double x = toScreen(Point2(static_cast<double>(i) * spacing, 0.0)).x();
        painter.drawLine(QPointF(x, 0.0), QPointF(x, height()));
    }
    for (long long i = firstIndex(visible.min.y); i <= lastIndex(visible.max.y); ++i) {
        painter.setPen(QPen(i % 5 == 0 ? kGridMajor : kGridMinor, 1));
        const double y = toScreen(Point2(0.0, static_cast<double>(i) * spacing)).y();
        painter.drawLine(QPointF(0.0, y), QPointF(width(), y));
    }
    const QPointF origin = toScreen(Point2(0.0, 0.0));
    painter.setPen(QPen(kAxis, 1));
    painter.drawLine(QPointF(origin.x(), 0.0), QPointF(origin.x(), height()));
    painter.drawLine(QPointF(0.0, origin.y()), QPointF(width(), origin.y()));
}

void ViewportWidget::drawEntities(QPainter& painter) const
{
    const auto& model = document_.model();
    const Box2 visible = view_.visibleWorldBounds();
    const cad::SelectionSet& selection = document_.selection();

    model.entities.forEach([&](const Entity& entity) {
        if (!cad::isDrawn(model, entity) ||
            !katana::entity::boundingBox(entity.geometry).intersects(visible)) {
            return;
        }
        const katana::entity::Layer* layer = model.layers.find(entity.layer);
        if (selection.contains(entity.id)) {
            painter.setPen(QPen(kSelection, 2, Qt::DashLine));
        } else {
            QColor color = toQColor(entity.color ? *entity.color : layer->color);
            if (layer->locked) {
                color.setAlpha(110); // locked layers read as background
            }
            painter.setPen(QPen(color, 1.5));
        }
        drawGeometry(painter, entity.geometry);
    });
}

void ViewportWidget::drawGeometry(QPainter& painter,
                                  const katana::entity::Geometry& geometry) const
{
    // Arcs are tessellated in model space so that very large radii, where only a
    // sliver is on screen, never hand QPainter coordinates in the millions.
    const auto drawArcPath = [&](const Arc2& arc) {
        const double radiusPixels = arc.radius * view_.scale;
        // Chord count for a sagitta under a quarter pixel, within sane bounds.
        const double stepAngle = radiusPixels > 1.0
                                     ? 2.0 * std::acos(std::max(0.0, 1.0 - 0.25 / radiusPixels))
                                     : katana::math::kPi;
        const int segments = static_cast<int>(
            std::clamp(std::ceil(std::abs(arc.sweep) / std::max(stepAngle, 1e-4)), 8.0, 2048.0));
        QPolygonF polygon;
        polygon.reserve(segments + 1);
        for (int i = 0; i <= segments; ++i) {
            polygon << toScreen(arc.pointAt(static_cast<double>(i) / segments));
        }
        painter.drawPolyline(polygon);
    };

    struct Visitor {
        const ViewportWidget& widget;
        QPainter& painter;
        const decltype(drawArcPath)& arcPath;

        void operator()(const katana::entity::PointGeometry& g) const
        {
            const QPointF p = widget.toScreen(g.position);
            const double r = kPointMarkerPixels;
            painter.drawLine(p + QPointF(-r, 0), p + QPointF(r, 0));
            painter.drawLine(p + QPointF(0, -r), p + QPointF(0, r));
        }
        void operator()(const Segment2& g) const
        {
            painter.drawLine(widget.toScreen(g.start), widget.toScreen(g.end));
        }
        void operator()(const Arc2& g) const { arcPath(g); }
        void operator()(const Circle2& g) const
        {
            arcPath(Arc2{g.center, g.radius, 0.0, katana::math::kTwoPi});
        }
        void operator()(const Polyline2& g) const
        {
            QPolygonF polygon;
            polygon.reserve(static_cast<int>(g.vertices.size()) + 1);
            for (const auto& vertex : g.vertices) {
                polygon << widget.toScreen(vertex);
            }
            if (g.closed && !g.vertices.empty()) {
                polygon << widget.toScreen(g.vertices.front());
            }
            painter.drawPolyline(polygon);
        }
        void operator()(const katana::entity::TextGeometry& g) const
        {
            widget.drawText(painter, g.position, g.text, g.height, g.rotation);
        }
        void operator()(const katana::entity::DimensionGeometry& g) const
        {
            const Vec2 along = (g.end - g.start).normalized();
            const Vec2 shift = along.perpendicular() * g.offset;
            const auto a = g.start + shift;
            const auto b = g.end + shift;
            painter.drawLine(widget.toScreen(g.start), widget.toScreen(a)); // extension lines
            painter.drawLine(widget.toScreen(g.end), widget.toScreen(b));
            painter.drawLine(widget.toScreen(a), widget.toScreen(b));       // dimension line
            // 45 degree ticks of fixed screen size at both ends.
            const QPointF dir(along.x, -along.y);
            const QPointF tick = (dir + QPointF(dir.y(), -dir.x())) * 5.0;
            for (const auto& end : {a, b}) {
                painter.drawLine(widget.toScreen(end) - tick, widget.toScreen(end) + tick);
            }
            const std::string label =
                g.textOverride.empty()
                    ? QString::number(g.measurement(), 'f', 3).toStdString()
                    : g.textOverride;
            const double height = 12.0 / widget.view_.scale; // constant 12 px label
            const auto anchor = (a + b) * 0.5 + along.perpendicular() * (0.4 * height) -
                                along * (0.3 * height * static_cast<double>(label.size()));
            widget.drawText(painter, anchor, label, height, along.angle());
        }
    };
    std::visit(Visitor{*this, painter, drawArcPath}, geometry);
}

void ViewportWidget::drawText(QPainter& painter, const Point2& position, const std::string& text,
                              double height, double rotation) const
{
    const double pixels = height * view_.scale;
    const QPointF anchor = toScreen(position);
    if (pixels < 3.0) {
        // Too small to read: a stroke along the baseline keeps it discoverable.
        const double width = 0.6 * pixels * static_cast<double>(text.size());
        painter.drawLine(anchor, anchor + QPointF(std::cos(rotation), -std::sin(rotation)) * width);
        return;
    }
    QFont font("Segoe UI");
    font.setPixelSize(static_cast<int>(std::min(pixels, 2000.0)));
    painter.save();
    painter.setFont(font);
    painter.translate(anchor);
    painter.rotate(-rotation * katana::math::kRadToDeg); // screen y points down
    painter.drawText(QPointF(0.0, 0.0), QString::fromStdString(text));
    painter.restore();
}

void ViewportWidget::drawPreview(QPainter& painter) const
{
    if (points_.empty()) {
        return;
    }
    painter.setPen(QPen(kPreview, 1, Qt::DashLine));
    const QPointF cursor = toScreen(cursorWorld_);
    switch (tool_) {
    case Tool::Line:
    case Tool::Move:
    case Tool::Copy:
        painter.drawLine(toScreen(points_.back()), cursor);
        break;
    case Tool::Polyline: {
        QPolygonF polygon;
        for (const auto& p : points_) {
            polygon << toScreen(p);
        }
        polygon << cursor;
        painter.drawPolyline(polygon);
        break;
    }
    case Tool::Rectangle:
        painter.drawRect(QRectF(toScreen(points_[0]), cursor).normalized());
        break;
    case Tool::Circle: {
        const double radius = points_[0].distanceTo(cursorWorld_) * view_.scale;
        painter.drawEllipse(toScreen(points_[0]), radius, radius);
        painter.drawLine(toScreen(points_[0]), cursor);
        break;
    }
    case Tool::Arc:
        if (points_.size() == 2) {
            if (const auto arc = Arc2::throughPoints(points_[0], points_[1], cursorWorld_)) {
                drawGeometry(painter, *arc);
                break;
            }
        }
        painter.drawLine(toScreen(points_.back()), cursor);
        break;
    default:
        break;
    }
}

void ViewportWidget::drawSnapMarker(QPainter& painter) const
{
    if (!activeSnap_ || activeSnap_->mode == cad::SnapMode::Grid) {
        return;
    }
    const QPointF p = toScreen(activeSnap_->point);
    const double r = 6.0;
    painter.setPen(QPen(kSnapMarker, 2));
    painter.setBrush(Qt::NoBrush);
    switch (activeSnap_->mode) {
    case cad::SnapMode::Endpoint:
        painter.drawRect(QRectF(p.x() - r, p.y() - r, 2 * r, 2 * r));
        break;
    case cad::SnapMode::Midpoint: {
        QPolygonF triangle;
        triangle << p + QPointF(0, -r) << p + QPointF(r, r) << p + QPointF(-r, r);
        painter.drawPolygon(triangle);
        break;
    }
    case cad::SnapMode::Center:
        painter.drawEllipse(p, r, r);
        break;
    case cad::SnapMode::Intersection:
        painter.drawLine(p + QPointF(-r, -r), p + QPointF(r, r));
        painter.drawLine(p + QPointF(-r, r), p + QPointF(r, -r));
        break;
    default: { // perpendicular, tangent, nearest: a diamond
        QPolygonF diamond;
        diamond << p + QPointF(0, -r) << p + QPointF(r, 0) << p + QPointF(0, r) << p + QPointF(-r, 0);
        painter.drawPolygon(diamond);
        break;
    }
    }
    painter.setFont(QFont("Segoe UI", 8));
    painter.drawText(p + QPointF(r + 4, -r - 2), cad::toString(activeSnap_->mode));
}

} // namespace katana::qt
