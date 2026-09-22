#include "viewport_widget.hpp"

#include "katana/cad/spatial_query.hpp"
#include "katana/cad/dashing.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/cad/hatching.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/entity/display.hpp"

#include <algorithm>
#include <optional>
#include <charconv>
#include <cmath>
#include <cstdio>
#include <limits>

#include <QPdfWriter>
#include <QPageSize>
#include <QLineF>
#include <QMarginsF>
#include <QFont>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPolygonF>
#include <QWheelEvent>

#include "katana/cad/selection.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/cad/symbols.hpp"
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
    documentListener_ = document_.addListener([this] { update(); });
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
    // Alignments are drawn but are not entities, so they are in no entity
    // bound. Found by looking at a screenshot: the sample's access road ran off
    // the bottom of a view that claimed to show everything.
    for (const katana::entity::Alignment& alignment : document_.model().alignments.all()) {
        if (const auto solved = katana::geometry::solveAlignment(alignment.horizontal)) {
            // A metre is fine enough for a bounding box; the curve cannot
            // stray further than that from its chords.
            for (const Point2& vertex : solved->toPolyline(1.0).vertices) {
                bounds.expand(vertex);
            }
        }
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
        // Through the Document's spatial index (PLAN.MD Phase 18). Measured in
        // Release on 100 000 entities: 4404 us per mouse move scanning,
        // 88 us indexed. The answer is identical either way - asserted by
        // IndexedQueries - so this is purely the cost.
        activeSnap_ = cad::snap(document_.model(), request, &document_.spatialIndex());
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
                                        view_.pixelsToWorld(kPickAperturePixels), {},
                                        &document_.spatialIndex());
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
    const auto picked = cad::pickInBox(document_.model(), box, mode, {},
                                       &document_.spatialIndex());
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
    // Beneath the drawing, like a surface: a mesh is context for what is
    // drawn over it, not a thing to be picked in plan.
    drawMeshFootprints(painter);
    drawEntities(painter);
    drawAlignments(painter);
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

    // Through the spatial index (PLAN.MD Phase 18). This runs on EVERY repaint
    // - every pan, every zoom - not just on a click, so it is the scan that
    // mattered most. Measured in Release at 500 000 entities zoomed to 1% of
    // the extent: 22.3 ms scanning, 0.090 ms indexed. forEachCandidate falls
    // back to the ordered scan for a zoomed-out repaint, where asking the
    // index for everything would be slower than walking the model once.
    // A dash pattern is a function of the linetype, the pen width and the
    // VIEW SCALE. The scale is fixed for a frame and changes between them,
    // so the cache lives exactly one frame.
    dashCache_.clear();
    std::vector<katana::geometry::SpatialId> scratch;
    cad::detail::forEachCandidate(
        model, &document_.spatialIndex(), visible, scratch, [&](const Entity& entity) {
        // The layer is found ONCE and the visibility rule is asked about
        // that layer, rather than looking it up again inside isDrawn.
        const katana::entity::Layer* layer = model.layers.find(entity.layer);
        // This box test is NOT the one forEachCandidate already did:
        // queryExtents is deliberately wider than the geometry (an arc offers
        // its centre for snapping), so this is the tighter, drawing-specific
        // filter and removing it would paint entities that are off screen.
        if (!cad::isDrawn(layer, entity) ||
            !katana::entity::boundingBox(entity.geometry).intersects(visible)) {
            return;
        }
        // Through the one resolution chain, so this agrees with the 3D view
        // and so that a named style can finally change how an entity looks -
        // Style::color was stored and validated and read by nothing.
        const auto display = katana::entity::resolveDisplay(model, entity);
        if (selection.contains(entity.id)) {
            painter.setPen(QPen(kSelection, 2, Qt::DashLine));
        } else {
            QColor color = toQColor(display.color);
            if (layer->locked) {
                color.setAlpha(110); // locked layers read as background
            }
            // On screen every line is a 1.5 px hairline: a screen has no
            // paper for a line weight to be millimetres of. On a plot the
            // width is Layer::lineWeight - "millimetres on paper" - which
            // means what it says for the first time (PLAN.MD Phase 22).
            const double penWidthPixels = paperPixelsPerMillimetre_ > 0.0
                                              ? display.lineWeight * paperPixelsPerMillimetre_
                                              : 1.5;
            QPen pen(color, penWidthPixels);
            // Dashes are MODEL lengths: a 0.5 m dash stays half a metre of
            // ground at every zoom, so the pixel pattern is recomputed from
            // the view scale each frame. Qt's array is in units of PEN WIDTH,
            // not pixels, which is why the width is passed in rather than
            // assumed - at 1.5 px a pattern that forgot it would be half again
            // too long.
            // The pattern depends only on the linetype, the view scale and
            // the pen width, and the view scale is fixed for a whole frame.
            // Computing it per entity rebuilt the same handful of patterns
            // tens of thousands of times a frame and allocated twice for each.
            if (const auto* linetype = model.linetypes.find(display.linetype);
                linetype != nullptr) {
                const auto key = std::make_pair(display.linetype, penWidthPixels);
                auto cached = dashCache_.find(key);
                if (cached == dashCache_.end()) {
                    cad::DashOptions dash;
                    dash.viewScale = view_.scale;
                    const auto pattern = cad::qtDashPattern(*linetype, dash, penWidthPixels);
                    cached = dashCache_.emplace(key, QList<qreal>(pattern.begin(), pattern.end()))
                                 .first;
                }
                if (!cached->second.isEmpty()) {
                    pen.setDashPattern(cached->second);
                }
            }
            painter.setPen(pen);
        }
        // Resolved once above and reused: resolveHatchPattern used to do a
        // second full resolveDisplay of its own, and the dimension style was
        // looked up for every entity although only a dimension can use it.
        hatch_ = cad::resolveHatchPattern(model, display);
        if (std::holds_alternative<katana::entity::DimensionGeometry>(entity.geometry)) {
            dimensionStyle_ = cad::resolveDimensionStyle(model, entity);
        }
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry);
            point != nullptr && !display.symbol.empty()) {
            drawSymbol(painter, display.symbol, point->position, display.symbolSize);
            return;
        }
        // A 12d linestyle IS the line, gaps and all: "move 0 0 / draw 3 0 /
        // move 5 0" is a three-unit dash followed by a two-unit gap, and a
        // fence style carries the fence as well as its ticks. So it REPLACES
        // the plain line rather than being drawn over it. Drawing both filled
        // in every gap, which made every linestyle look continuous.
        const auto* definition = document_.definitionFor(display.linetype);
        const bool drawnByStyle = definition != nullptr && !definition->atVertices &&
                                  drawLineStyle(painter, *definition, entity.geometry);
        // A hatch is painted inside drawGeometry, so an entity carrying one
        // is drawn anyway and puts up with a doubled outline. A 12da never
        // brings a hatch; this is for a drawing given one in Katana.
        if (!drawnByStyle || hatch_ != nullptr) {
            drawGeometry(painter, entity.geometry);
        }
    });
}

void ViewportWidget::setMeshes(const std::vector<katana::cad::SceneMesh>* meshes)
{
    meshes_ = meshes;
    update();
}

// A mesh in plan is its FOOTPRINT - the hull of its vertices - and not its
// triangles: 1 453 meshes of 90 656 triangles arrive from one real archive,
// and drawing those in plan would bury the drawing they are context for. The
// 3D view is where a mesh is looked at.
void ViewportWidget::drawMeshFootprints(QPainter& painter) const
{
    if (meshes_ == nullptr) {
        return;
    }
    for (const katana::cad::SceneMesh& item : *meshes_) {
        if (!item.visible || item.mesh == nullptr || item.mesh->empty() ||
            item.style == cad::SurfaceStyle::Hidden) {
            continue;
        }
        const auto hull = item.mesh->planHull();
        if (hull.size() < 2) {
            continue;
        }
        const QColor colour(katana::render::redOf(item.flatColor),
                            katana::render::greenOf(item.flatColor),
                            katana::render::blueOf(item.flatColor));
        QColor outline = colour;
        outline.setAlpha(190);
        painter.setPen(QPen(outline, 1, Qt::DashLine));
        QColor fill = colour;
        fill.setAlpha(40);
        QPolygonF polygon;
        polygon.reserve(static_cast<int>(hull.size()) + 1);
        for (const auto& vertex : hull) {
            polygon << toScreen(vertex);
        }
        // Two points are a wall seen from above: a line, with nothing to fill.
        if (hull.size() == 2) {
            painter.drawPolyline(polygon);
            continue;
        }
        painter.setBrush(fill);
        painter.drawPolygon(polygon);
        painter.setBrush(Qt::NoBrush);
    }
}

void ViewportWidget::drawStyleDrawing(QPainter& painter, const cad::StyleDrawing& drawing) const
{
    // A definition can change pen part way through - `colour "pen 035"` - and
    // an empty pen means the entity's own colour, which is whatever the
    // painter already carries.
    const QPen entityPen = painter.pen();
    for (const auto& stroke : drawing.strokes) {
        painter.setPen(penFor(entityPen, stroke.pen));
        if (stroke.path.vertices.size() == 1) {
            painter.drawPoint(toScreen(stroke.path.vertices.front())); // a `dot`
            continue;
        }
        QPolygonF polygon;
        polygon.reserve(static_cast<int>(stroke.path.vertices.size()) + 1);
        for (const auto& vertex : stroke.path.vertices) {
            polygon << toScreen(vertex);
        }
        if (stroke.path.closed && !stroke.path.vertices.empty()) {
            polygon << toScreen(stroke.path.vertices.front());
        }
        painter.drawPolyline(polygon);
    }
    for (const auto& text : drawing.texts) {
        painter.setPen(penFor(entityPen, text.pen));
        drawStyleText(painter, text);
    }
    painter.setPen(entityPen);
}

QPen ViewportWidget::penFor(const QPen& entityPen, const std::string& pen)
{
    if (pen.empty()) {
        return entityPen; // 12d's "view_colour": whatever the entity is
    }
    QPen changed = entityPen;
    // 12d's standard colour names, which the archive reader already knows. A
    // pen this does not know keeps the entity's own colour rather than
    // guessing at one.
    if (const auto colour = katana::archive12d::standardColour(pen); colour) {
        changed.setColor(QColor(colour->r, colour->g, colour->b));
    }
    return changed;
}

double ViewportWidget::paperScale() const
{
    // Model units to one plot millimetre, which is what a `paperstyle` is
    // measured in. Dividing by the view scale is what makes such a mark keep
    // its size on the PAGE as you zoom, which is the whole point of one.
    //
    // With no plot scale set, a millimetre is a millimetre OF SCREEN. It used
    // to be one pixel, which made every paper linestyle about four times too
    // small: the ticks of a fence style came out a pixel tall and vanished
    // into the line they sit on, so the feature looked broken when it was
    // only invisible.
    const double pixelsPerMillimetre = paperPixelsPerMillimetre_ > 0.0
                                           ? paperPixelsPerMillimetre_
                                           : std::max(1.0, logicalDpiX() / 25.4);
    return pixelsPerMillimetre / std::max(view_.scale, 1e-12);
}

void ViewportWidget::drawStyleText(QPainter& painter, const cad::StyleTextMark& text) const
{
    if (text.text.empty() || text.height <= 0.0) {
        return;
    }
    const double pixels = text.height * view_.scale;
    if (pixels < 3.0) {
        return; // smaller than it is worth painting, and unreadable anyway
    }
    painter.save();
    painter.translate(toScreen(text.at));
    // Screen y grows downwards, so a counter-clockwise model angle turns the
    // other way on the page.
    painter.rotate(-text.angle * 180.0 / std::numbers::pi);
    QFont font = painter.font();
    font.setPixelSize(std::max(1, static_cast<int>(std::lround(pixels))));
    if (!text.font.empty()) {
        font.setFamily(QString::fromStdString(text.font));
    }
    painter.setFont(font);
    const QString value = QString::fromStdString(text.text);
    const QFontMetricsF metrics(font);
    // 12d justifies as "vertical-horizontal": "middle-centre", "top-left".
    // A spelling this does not know draws from the point, which is what an
    // unjustified text already does.
    double dx = 0.0;
    double dy = 0.0;
    if (text.justify.find("centre") != std::string::npos ||
        text.justify.find("center") != std::string::npos) {
        dx = -0.5 * metrics.horizontalAdvance(value);
    } else if (text.justify.find("right") != std::string::npos) {
        dx = -metrics.horizontalAdvance(value);
    }
    if (text.justify.find("middle") != std::string::npos) {
        dy = 0.5 * metrics.capHeight();
    } else if (text.justify.find("top") != std::string::npos) {
        dy = metrics.capHeight();
    }
    painter.drawText(QPointF(dx, dy), value);
    painter.restore();
}

// A linestyle runs along whatever plan shape the entity has. An arc and a
// circle are chorded first, because a pattern is laid by distance along a
// path and a path is what a polyline is.
bool ViewportWidget::drawLineStyle(QPainter& painter,
                                   const katana::entity::LineStyle& definition,
                                   const katana::entity::Geometry& geometry) const
{
    bool drew = false;
    const double scale = paperScale();
    // A quarter of a pixel, the same accuracy drawGeometry chords to, so a
    // pattern laid along a curve follows the curve that was drawn.
    const double chordTolerance = 0.25 / std::max(view_.scale, 1e-12);
    const auto run = [&](const katana::geometry::Polyline2& shape) {
        if (shape.vertices.size() < 2) {
            return;
        }
        const cad::StyleDrawing drawing = cad::styleDrawing(definition, shape, scale);
        if (drawing.empty()) {
            return; // nothing came of it; the caller draws the plain line
        }
        drawStyleDrawing(painter, drawing);
        drew = true;
    };
    std::visit(
        [&](const auto& shape) {
            using T = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<T, katana::geometry::Segment2>) {
                run(katana::geometry::Polyline2{{shape.start, shape.end}, false});
            } else if constexpr (std::is_same_v<T, katana::geometry::Polyline2>) {
                run(shape);
            } else if constexpr (std::is_same_v<T, katana::geometry::Arc2>) {
                run(katana::geometry::Polyline2{
                    katana::geometry::chordArc(shape, chordTolerance), false});
            } else if constexpr (std::is_same_v<T, katana::geometry::Circle2>) {
                run(katana::geometry::Polyline2{
                    katana::geometry::chordCircle(shape, chordTolerance), true});
            }
            // A point, a text and a mesh have no line to lay a pattern along.
        },
        geometry);
    return drew;
}

void ViewportWidget::drawSymbol(QPainter& painter, const std::string& symbol,
                                const Point2& centre, double size) const
{
    // A loaded 12d symbol library answers first; the sixteen built-in shapes
    // are what a name falls back to (PLAN.MD 20.3).
    if (const auto* definition = document_.definitionFor(symbol); definition != nullptr) {
        drawStyleDrawing(painter, cad::symbolDrawing(*definition, centre, size, 0.0, paperScale()));
        return;
    }
    // The style's size is the symbol's width, as 12d's is; the strokes take
    // a half-width. A symbol with no size of its own is the plain mark's
    // size, in model units at the current scale, so it stays a mark and not
    // a blob.
    const double half =
        size > 0.0 ? 0.5 * size : kPointMarkerPixels / std::max(view_.scale, 1e-12);
    for (const auto& stroke : cad::symbolStrokes(symbol, centre, half)) {
        QPolygonF polygon;
        polygon.reserve(static_cast<int>(stroke.vertices.size()) + 1);
        for (const auto& vertex : stroke.vertices) {
            polygon << toScreen(vertex);
        }
        if (stroke.closed && !stroke.vertices.empty()) {
            polygon << toScreen(stroke.vertices.front());
        }
        painter.drawPolyline(polygon);
    }
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
                // The fill goes down before the boundary, so the outline stays
                // crisp over its own hatching instead of being half covered.
                widget.drawHatch(painter, g, polygon);
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
            // Through the shared builder, so this draws exactly what the 3D
            // view draws and exactly what the cull box covers.
            //
            // Everything is in MODEL units now. The previous version drew a
            // fixed 5-pixel tick and a 12-pixel label, which looks right on
            // screen and plots at whatever size the paper happens to give it -
            // a dimension is part of the drawing, not an overlay on it. It also
            // formatted the number with QString::number, which is LOCALE
            // DEPENDENT and would put a comma in "1,5" on a European machine.
            const auto drawing = cad::buildDimension(g, widget.dimensionStyle_);
            if (drawing.empty()) {
                return;
            }
            const auto line = [this](const Segment2& segment) {
                painter.drawLine(widget.toScreen(segment.start), widget.toScreen(segment.end));
            };
            for (const Segment2& segment : drawing.extensionLines) {
                line(segment);
            }
            line(drawing.dimensionLine);
            for (const Segment2& stroke : drawing.arrowStrokes) {
                line(stroke);
            }
            for (const std::vector<Point2>& fill : drawing.arrowFills) {
                QPolygonF polygon;
                polygon.reserve(static_cast<int>(fill.size()));
                for (const Point2& point : fill) {
                    polygon << widget.toScreen(point);
                }
                const QBrush previous = painter.brush();
                painter.setBrush(painter.pen().color());
                painter.drawPolygon(polygon);
                painter.setBrush(previous);
            }
            widget.drawText(painter, drawing.textAnchor, drawing.text, drawing.textHeight,
                            drawing.textRotation);
        }
    };
    std::visit(Visitor{*this, painter, drawArcPath}, geometry);
}

namespace {

// Chainage in the civil convention, kilometres + metres: 1234.5 reads as
// "1+234.50". Formatted with to_chars rather than snprintf, because snprintf
// obeys the C locale and would print "1+234,50" on a machine set to one that
// uses a decimal comma - the same reason the dimension formatter avoids it.
std::string formatStation(double station)
{
    const double magnitude = std::abs(station);
    const auto kilometres = static_cast<long long>(magnitude / 1000.0);
    const double metres = magnitude - static_cast<double>(kilometres) * 1000.0;
    char buffer[32];
    const auto result =
        std::to_chars(buffer, buffer + sizeof buffer, metres, std::chars_format::fixed, 2);
    std::string metresText(buffer, result.ptr);
    while (metresText.size() < 6) { // "000.00"
        metresText.insert(metresText.begin(), '0');
    }
    return (station < 0.0 ? "-" : "") + std::to_string(kilometres) + "+" + metresText;
}

} // namespace

void ViewportWidget::drawAlignments(QPainter& painter) const
{
    const auto& model = document_.model();
    if (model.alignments.empty() || !(view_.scale > 0.0)) {
        return;
    }
    // Half a pixel: finer cannot be seen, coarser shows facets on tight curves.
    const double tolerance = 0.5 / view_.scale;
    const Box2 visible = view_.visibleWorldBounds();
    const QColor kAlignment(0xff, 0xb7, 0x4d); // amber: an overlay, not drawing content
    const double tick = 6.0 / view_.scale;     // screen-constant, like the snap marker
    const double height = 11.0 / view_.scale;

    for (const katana::entity::Alignment& alignment : model.alignments.all()) {
        // Solved per repaint. A document has a handful of alignments and the
        // solve is a few spiral end-points; caching it would need invalidation
        // on every edit for no measurable gain. Measure before changing this.
        const auto solved = katana::geometry::solveAlignment(alignment.horizontal);
        if (!solved) {
            continue; // the model refused it on the way in; nothing to draw
        }
        const Polyline2 line = solved->toPolyline(tolerance);
        if (line.vertices.size() < 2) {
            continue;
        }
        Box2 box;
        for (const Point2& vertex : line.vertices) {
            box.expand(vertex);
        }
        if (!box.inflated(tick * 4.0).intersects(visible)) {
            continue;
        }

        QPolygonF polygon;
        polygon.reserve(static_cast<int>(line.vertices.size()));
        for (const Point2& vertex : line.vertices) {
            polygon << toScreen(vertex);
        }
        painter.setPen(QPen(kAlignment, 2.0));
        painter.drawPolyline(polygon);

        painter.setPen(QPen(kAlignment, 1.0));
        // Key stations bunch up - a 10 m spiral puts TS and SC ten metres
        // apart - and their labels then print over one another into a smear
        // nobody can read. A label is skipped when it would land within its
        // own length of the last one drawn; the TICK is always drawn, because
        // the tick is the information and the label only names it.
        std::optional<QPointF> lastLabel;
        const double minimumLabelGapPixels = 70.0; // about one "0+000.00" at this text size
        for (const double station : solved->keyStations()) {
            const auto left = solved->pointAtStationOffset(station, tick);
            const auto right = solved->pointAtStationOffset(station, -tick);
            const auto direction = solved->directionAtStation(station);
            const auto label = solved->pointAtStationOffset(station, tick * 1.6);
            if (!left || !right || !direction || !label) {
                continue;
            }
            painter.drawLine(toScreen(*left), toScreen(*right));
            const QPointF at = toScreen(*label);
            if (lastLabel.has_value() &&
                QLineF(*lastLabel, at).length() < minimumLabelGapPixels) {
                continue;
            }
            lastLabel = at;
            drawText(painter, *label, formatStation(station), height, *direction);
        }
        if (const auto start = solved->pointAtStationOffset(solved->startStation(), -tick * 3.0)) {
            const auto direction = solved->directionAtStation(solved->startStation());
            drawText(painter, *start, alignment.name, height * 1.3, direction.value_or(0.0));
        }
    }
}

katana::core::Status ViewportWidget::plotToPdf(const QString& path,
                                               const cad::PlotSettings& settings)
{
    auto sheet = cad::sheetFor(settings);
    if (!sheet) {
        return sheet.error();
    }
    QPdfWriter writer(path);
    writer.setResolution(static_cast<int>(settings.dpi));
    const cad::PaperDimensions paper = cad::paperDimensions(settings.paper, settings.landscape);
    writer.setPageSize(QPageSize(QSizeF(paper.widthMm, paper.heightMm), QPageSize::Millimeter));
    // The sheet transform owns the margins; the writer's would shift the page.
    writer.setPageMargins(QMarginsF(0.0, 0.0, 0.0, 0.0));
    QPainter painter(&writer);
    if (!painter.isActive()) {
        return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                       "could not open the PDF for writing", path.toStdString());
    }

    // The same drawing members as paintEvent, through the sheet instead of
    // the screen, with line weights in paper millimetres - then the screen
    // view is put back exactly as it was.
    const cad::ViewTransform screen = view_;
    view_ = sheet->view;
    paperPixelsPerMillimetre_ = sheet->pixelsPerMillimetre;
    painter.setRenderHint(QPainter::Antialiasing, true);
    drawEntities(painter);
    drawAlignments(painter);
    painter.end();
    view_ = screen;
    paperPixelsPerMillimetre_ = 0.0;
    return {};
}

void ViewportWidget::drawHatch(QPainter& painter, const katana::geometry::Polyline2& boundary,
                                 const QPolygonF& screen) const
{
    if (hatch_ == nullptr) {
        return;
    }
    cad::HatchOptions options;
    options.viewScale = view_.scale;
    const QColor color = painter.pen().color();

    switch (cad::hatchDrawing(*hatch_, options)) {
    case cad::HatchDrawing::None:
        return;
    case cad::HatchDrawing::Solid: {
        // Drawn at partial opacity rather than flat: a solid fill in the
        // entity's own colour hides the drawing underneath it, and at this zoom
        // the user is looking at the layout, not at the fill.
        QColor fill = color;
        fill.setAlpha(90);
        painter.fillPath([&] {
            QPainterPath path;
            path.addPolygon(screen);
            path.closeSubpath();
            return path;
        }(), fill);
        return;
    }
    case cad::HatchDrawing::Lines:
        break;
    }

    // Hatch lines are always solid and hairline, whatever the boundary is
    // drawn with: a dashed hatch of a dashed boundary is unreadable, and no CAD
    // package draws one.
    const QPen previous = painter.pen();
    painter.setPen(QPen(color, 0));
    for (const Segment2& line : cad::hatchSegments(boundary, *hatch_)) {
        painter.drawLine(toScreen(line.start), toScreen(line.end));
    }
    painter.setPen(previous);
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
