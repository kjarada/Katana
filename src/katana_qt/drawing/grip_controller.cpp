#include "drawing/grip_controller.hpp"

#include <algorithm>

#include <QPainter>
#include <QPen>

#include "drawing/feedback_painter.hpp"
#include "katana/core/text.hpp"

namespace katana::qt::drawing {

namespace cad = katana::cad;

namespace {

// AutoCAD's grip colours, which a drafter's eye already reads - blue cold,
// green under the cursor, red hot - are the overlay's (feedback_painter.hpp),
// as is the rubber band's cyan, a tool preview's.
constexpr double kGripHalfPixels = 4.0;
constexpr double kDragPixels = 4.0;

} // namespace

GripController::GripController(cad::Document& document) : document_(document) {}

void GripController::refresh(std::uint64_t generation)
{
    if (generation == generation_) {
        return;
    }
    generation_ = generation;
    grips_ = cad::gripsOfSelection(document_, document_.selection().ids());
    // A hot grip survives a refresh only if its handle still exists (an
    // undo, a selection change or another view's edit may have taken it).
    std::erase_if(hot_, [&](const cad::Grip& hot) {
        return std::none_of(grips_.begin(), grips_.end(),
                            [&](const cad::Grip& grip) { return grip.sameHandle(hot); });
    });
    for (cad::Grip& hot : hot_) {
        for (const cad::Grip& grip : grips_) {
            if (grip.sameHandle(hot)) {
                hot.position = grip.position;
            }
        }
    }
    if (grabbed_ && std::none_of(grips_.begin(), grips_.end(),
                                 [&](const cad::Grip& grip) { return grip.sameHandle(*grabbed_); })) {
        grabbed_.reset();
        state_ = State::Idle;
        typed_.clear();
    }
}

std::optional<GripController::Point2> GripController::base() const
{
    return grabbed_ ? std::optional<Point2>(grabbed_->position) : std::nullopt;
}

QString GripController::prompt() const
{
    if (!grabbed_) {
        return {};
    }
    return QString("Grip %1: specify the point or type x,y, @dx,dy or @distance<angle  %2_")
        .arg(QString::fromUtf8(cad::toString(grabbed_->kind)), typed_);
}

bool GripController::isHot(const cad::Grip& grip) const
{
    return std::any_of(hot_.begin(), hot_.end(),
                       [&](const cad::Grip& hot) { return hot.sameHandle(grip); });
}

bool GripController::press(const Point2& at, double aperture, bool shift, bool ctrl)
{
    if (state_ == State::Attached) {
        // The grip was picked up by a click; this click puts it down, where
        // the cursor is (snapped and constrained by the view).
        commitAt(target_);
        return true;
    }
    const auto found = cad::gripAt(grips_, at, aperture);
    if (!found) {
        return false;
    }
    if (shift) {
        if (isHot(*found)) {
            std::erase_if(hot_, [&](const cad::Grip& hot) { return hot.sameHandle(*found); });
        } else {
            hot_.push_back(*found);
        }
        return true;
    }
    if (!isHot(*found)) {
        hot_ = {*found};
    }
    grabbed_ = *found;
    insert_ = ctrl;
    target_ = found->position;
    rawCursor_ = at;
    state_ = State::Pressed;
    return true;
}

void GripController::move(const Point2& target, const Point2& raw, double aperture,
                          double movedPixels)
{
    rawCursor_ = raw;
    if (state_ == State::Pressed && movedPixels > kDragPixels) {
        state_ = State::Dragging;
    }
    if (active()) {
        target_ = target;
        hovered_.reset();
        return;
    }
    hovered_ = cad::gripAt(grips_, raw, aperture);
}

bool GripController::release()
{
    if (state_ == State::Dragging) {
        commitAt(target_);
        return true;
    }
    if (state_ == State::Pressed) {
        // A click without a drag picks the grip up.
        state_ = State::Attached;
        return true;
    }
    return false;
}

bool GripController::escape()
{
    if (grabbed_) {
        grabbed_.reset();
        state_ = State::Idle;
        typed_.clear();
        insert_ = false;
        return true;
    }
    if (!hot_.empty()) {
        hot_.clear();
        return true;
    }
    return false;
}

bool GripController::deleteHot()
{
    const bool anyVertex = std::any_of(hot_.begin(), hot_.end(), [](const cad::Grip& grip) {
        return grip.kind == cad::GripKind::Vertex;
    });
    if (!anyVertex) {
        return false;
    }
    const auto status = document_.execute(cad::deleteHotVertices(hot_));
    if (!status && onError) {
        onError(QString::fromStdString(status.error().describe()));
    }
    hot_.clear();
    grabbed_.reset();
    state_ = State::Idle;
    return true;
}

bool GripController::enter()
{
    if (!grabbed_) {
        return false;
    }
    if (typed_.trimmed().isEmpty()) {
        commitAt(target_);
        return true;
    }
    const std::string text = typed_.trimmed().toStdString();
    typed_.clear();
    const auto& drafting = document_.drafting();
    if (cad::looksLikePoint(text)) {
        const auto point = cad::parsePrecisePoint(text, grabbed_->position, drafting);
        if (!point) {
            if (onError) {
                onError(QString::fromStdString(point.error().describe()));
            }
            return true;
        }
        commitAt(point->point);
        return true;
    }
    if (const auto distance = katana::core::parseFiniteDouble(text)) {
        // Direct distance entry: that far from where the grip was, towards
        // the cursor.
        commitAt(cad::directDistance(grabbed_->position, rawCursor_, *distance, drafting));
        return true;
    }
    if (onError) {
        onError(QString("'%1' is not a point or a distance; type x,y, @dx,dy, @distance<angle "
                        "or a distance")
                    .arg(QString::fromStdString(text)));
    }
    return true;
}

bool GripController::type(const QString& text)
{
    if (!grabbed_) {
        return false;
    }
    typed_ += text;
    return true;
}

bool GripController::backspace()
{
    if (!grabbed_) {
        return false;
    }
    typed_.chop(1);
    return true;
}

void GripController::reset()
{
    hot_.clear();
    grabbed_.reset();
    hovered_.reset();
    state_ = State::Idle;
    typed_.clear();
    insert_ = false;
    generation_ = ~std::uint64_t{0};
}

cad::GripDrag GripController::dragTo(const Point2& target) const
{
    cad::GripDrag drag;
    drag.grabbed = *grabbed_;
    drag.target = target;
    drag.insertVertex = insert_;
    for (const cad::Grip& hot : hot_) {
        if (!hot.sameHandle(*grabbed_)) {
            drag.alsoHot.push_back(hot);
        }
    }
    return drag;
}

void GripController::commitAt(const Point2& target)
{
    if (!grabbed_) {
        return;
    }
    const cad::GripDrag drag = dragTo(target);
    grabbed_.reset();
    state_ = State::Idle;
    typed_.clear();
    insert_ = false;
    hot_.clear();
    if (drag.target.distanceTo(drag.grabbed.position) <= katana::math::tolerance::kGeometric &&
        !drag.insertVertex) {
        return; // put down where it was: nothing to do, and no empty undo step
    }
    const auto status = document_.execute(cad::gripDragCommand(drag));
    if (!status && onError) {
        onError(QString::fromStdString(status.error().describe()));
    }
}

void GripController::paint(QPainter& painter, const std::function<QPointF(const Point2&)>& toScreen,
                           const std::function<void(const katana::entity::Geometry&)>& drawShape,
                           const QRectF& visible) const
{
    if (grabbed_ && active()) {
        const cad::GripDrag drag = dragTo(target_);
        painter.setPen(QPen(overlay::preview(), 1, Qt::DashLine));
        painter.setBrush(Qt::NoBrush);
        for (const auto& shape : cad::gripPreview(document_, drag)) {
            drawShape(shape);
        }
        painter.setPen(QPen(overlay::preview(), 1, Qt::DotLine));
        painter.drawLine(toScreen(grabbed_->position), toScreen(target_));
    }
    painter.setBrush(Qt::NoBrush);
    for (const cad::Grip& grip : grips_) {
        const QPointF p = toScreen(grip.position);
        if (!visible.contains(p)) {
            continue;
        }
        QColor colour = overlay::gripCold();
        if (isHot(grip)) {
            colour = overlay::gripHot();
        } else if (hovered_ && hovered_->sameHandle(grip)) {
            colour = overlay::gripHover();
        }
        painter.setPen(QPen(colour.darker(150), 1));
        painter.setBrush(colour);
        const double r = grip.kind == cad::GripKind::SegmentMid ? kGripHalfPixels - 1.0
                                                                : kGripHalfPixels;
        if (grip.kind == cad::GripKind::SegmentMid) {
            // A segment's middle is a smaller diamond, so it does not read as
            // a vertex at a glance.
            QPolygonF diamond;
            diamond << p + QPointF(0, -r - 1) << p + QPointF(r + 1, 0) << p + QPointF(0, r + 1)
                    << p + QPointF(-r - 1, 0);
            painter.drawPolygon(diamond);
        } else if (grip.kind == cad::GripKind::Centre) {
            painter.drawEllipse(p, r, r);
        } else {
            painter.drawRect(QRectF(p.x() - r, p.y() - r, 2 * r, 2 * r));
        }
    }
    painter.setBrush(Qt::NoBrush);
}

} // namespace katana::qt::drawing
