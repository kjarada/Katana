#include "drawing/grip_controller.hpp"

#include <algorithm>
#include <cstddef>
#include <optional>
#include <utility>
#include <vector>

#include <QPainter>
#include <QPen>

#include "drawing/feedback_painter.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

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

namespace {

// The places of `entity`'s vertex grips, by index; empty for an entity with
// none (a circle, a text).
std::vector<std::optional<katana::geometry::Point2>> vertexPlaces(
    const std::vector<cad::Grip>& grips, katana::entity::EntityId entity)
{
    std::vector<std::optional<katana::geometry::Point2>> places;
    for (const cad::Grip& grip : grips) {
        if (grip.entity != entity || grip.kind != cad::GripKind::Vertex) {
            continue;
        }
        if (grip.index >= places.size()) {
            places.resize(grip.index + 1);
        }
        places[grip.index] = grip.position;
    }
    return places;
}

// Whether a polyline grip's own vertices are all that moved from `before` to
// `grips`: every other vertex of its entity is where it was, index for index,
// and there are as many. A vertex grip's own vertex is itself; a segment
// middle's are the segment's two ends.
bool onlyItsOwnVerticesMoved(const cad::Grip& old, const std::vector<cad::Grip>& grips,
                             const std::vector<cad::Grip>& before)
{
    const auto was = vertexPlaces(before, old.entity);
    const auto now = vertexPlaces(grips, old.entity);
    if (was.size() != now.size() || now.empty()) {
        return false;
    }
    const std::size_t n = now.size();
    for (std::size_t i = 0; i < n; ++i) {
        const bool own = old.kind == cad::GripKind::Vertex
                             ? i == old.index
                             : i == old.index || i == (old.index + 1) % n;
        if (own) {
            continue;
        }
        if (!was[i] || !now[i] ||
            was[i]->distanceTo(*now[i]) > katana::math::tolerance::kGeometric) {
            return false;
        }
    }
    return true;
}

// The grip that is `old` after the drawing changed from `before` to `grips`:
// the same point of the same entity, by its position - an index names
// another vertex once a vertex is inserted or deleted before it, and the next
// Delete would take that one. The same index first, where a twin vertex lies
// on the same point.
//
// Where no grip is at its point, its own vertex may have been moved (VERTEX
// MOVE, a Vertices panel cell, another view), and matched by position alone
// such a move silently unchose it. A polyline's vertex or segment middle then
// keeps its index only when every OTHER vertex of the polyline is where it
// was: that is a move of its own. Two edits between refreshes - a SCRIPT, lines
// pasted, an agent - can delete the chosen vertex and insert another,
// leaving as many vertices as before; kept by its index then, the grip named a
// vertex nobody chose, and Enter or Delete removed it. It goes instead. A
// grip of any other kind (a centre, a quadrant, a line's end) keeps its index
// while its entity has as many of that kind: those are never renumbered.
std::optional<cad::Grip> follow(const cad::Grip& old, const std::vector<cad::Grip>& grips,
                                const std::vector<cad::Grip>& before)
{
    const auto same = [&](const cad::Grip& grip) {
        return grip.entity == old.entity && grip.kind == old.kind;
    };
    const auto at = [&](const cad::Grip& grip) {
        return same(grip) &&
               grip.position.distanceTo(old.position) <= katana::math::tolerance::kGeometric;
    };
    for (const cad::Grip& grip : grips) {
        if (grip.index == old.index && at(grip)) {
            return grip;
        }
    }
    for (const cad::Grip& grip : grips) {
        if (at(grip)) {
            return grip;
        }
    }
    const bool renumbered =
        old.kind == cad::GripKind::Vertex || old.kind == cad::GripKind::SegmentMid;
    const bool kept = renumbered
                          ? onlyItsOwnVerticesMoved(old, grips, before)
                          : std::ranges::count_if(grips, same) == std::ranges::count_if(before, same);
    if (kept) {
        for (const cad::Grip& grip : grips) {
            if (grip.index == old.index && same(grip)) {
                return grip;
            }
        }
    }
    return std::nullopt;
}

} // namespace

void GripController::refresh(std::uint64_t generation)
{
    const cad::LayerOverrides& view = view_ != nullptr ? *view_ : cad::kNoLayerOverrides;
    if (generation == generation_ && view == builtFor_) {
        return;
    }
    generation_ = generation;
    builtFor_ = view;
    const std::vector<cad::Grip> before = std::exchange(
        grips_, cad::gripsOfSelection(document_, document_.selection().ids(), view));
    // A hot grip survives a refresh only while its vertex does (an undo, a
    // selection change or another view's edit may have taken it), and it
    // follows its vertex to the index it has now (follow).
    std::vector<cad::Grip> kept;
    for (const cad::Grip& hot : hot_) {
        if (auto now = follow(hot, grips_, before)) {
            kept.push_back(*now);
        }
    }
    hot_ = std::move(kept);
    if (grabbed_) {
        if (auto now = follow(*grabbed_, grips_, before)) {
            grabbed_ = *now;
        } else {
            grabbed_.reset();
            state_ = State::Idle;
            typed_.clear();
        }
    }
}

bool GripController::hasHotVertex() const
{
    return std::any_of(hot_.begin(), hot_.end(),
                       [](const cad::Grip& grip) { return grip.kind == cad::GripKind::Vertex; });
}

QString GripController::hoverHint() const
{
    if (!hovered_ || grabbed_) {
        return {};
    }
    const cad::Grip& grip = *hovered_;
    const katana::entity::Entity* entity = document_.model().entities.find(grip.entity);
    if (entity == nullptr) {
        return {};
    }
    // A polyline's vertex and segment middle, the grips vertex editing is
    // about, say what else they offer: the shortcut menu's items, and the
    // gestures nothing else on screen mentions (Shift to choose, Ctrl to add).
    if (const auto polyline = cad::readPolyline(*entity)) {
        if (grip.kind == cad::GripKind::Vertex && grip.index < polyline->vertices.size()) {
            // Its height written as the vertex tools' labels write it
            // (cad::heightText): "z -0.000" here beside "z 0.000" there was
            // one vertex read two ways.
            const auto& height = polyline->vertices[grip.index].height;
            return QString("Vertex %1 of polyline %2%3: drag to move · click to pick up · "
                           "Shift+click to choose · Delete removes the chosen · right-click for "
                           "vertex tools")
                .arg(grip.index)
                .arg(grip.entity)
                .arg(height ? ", " + QString::fromStdString(cad::heightText(height)) : QString());
        }
        if (grip.kind == cad::GripKind::SegmentMid) {
            // A straight segment's middle moves the segment bodily; an arc's
            // keeps both ends and bends the arc through the cursor
            // (docs/drawing.md, "What a drag means per handle").
            const bool arc =
                grip.index < polyline->segmentCount() && polyline->isArc(grip.index);
            return QString("Segment %1 of polyline %2: drag to %3 · Ctrl+drag to add a vertex "
                           "· right-click for segment tools")
                .arg(grip.index)
                .arg(grip.entity)
                .arg(arc ? QStringLiteral("bend the arc") : QStringLiteral("stretch"));
        }
    }
    QString kind = QString::fromUtf8(cad::toString(grip.kind));
    if (!kind.isEmpty()) {
        kind[0] = kind[0].toUpper();
    }
    return QString("%1 of %2 %3: drag to move it · click to pick it up · Shift+click to choose")
        .arg(kind, QString::fromUtf8(std::string(katana::entity::toString(entity->type()))))
        .arg(grip.entity);
}

void GripController::setHot(std::vector<cad::Grip> grips)
{
    grabbed_.reset();
    state_ = State::Idle;
    typed_.clear();
    insert_ = false;
    hot_ = std::move(grips);
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
    // A Ctrl-press on a segment middle adds a vertex where it is put down;
    // the band said what a stretch says, word for word.
    if (insert_ && grabbed_->kind == cad::GripKind::SegmentMid) {
        return QString("Grip segment middle, adding a vertex: specify where it goes or type x,y, "
                       "@dx,dy or @distance<angle  %1_")
            .arg(typed_);
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
    FeedbackFrame frame;
    frame.toScreen = toScreen;
    frame.drawShape = drawShape;
    frame.visible = visible;
    if (grabbed_ && active()) {
        // The drag as a tool's preview is drawn (cad::gripFeedback): the
        // edited geometry dashed, the grip's old place, and the vertex a
        // Ctrl-drag adds.
        const cad::GripDrag drag = dragTo(target_);
        frame.cursor = toScreen(target_);
        (void)paintFeedback(painter, cad::gripFeedback(document_, drag), {}, frame);
        painter.setPen(QPen(overlay::preview(), 1, Qt::DotLine));
        painter.setBrush(Qt::NoBrush);
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
    // Ctrl held over a segment middle: it is drawn as the vertex a drag
    // there adds, the gesture's only sign on screen.
    if (ctrl_ && hovered_ && hovered_->kind == cad::GripKind::SegmentMid && !active()) {
        cad::ToolFeedback added;
        added.marks.push_back(cad::FeedbackMark{
            cad::FeedbackRole::Added,
            katana::entity::Geometry{katana::entity::PointGeometry{hovered_->position}}, {}});
        frame.cursor = toScreen(hovered_->position);
        (void)paintFeedback(painter, added, {}, frame);
    }
}

} // namespace katana::qt::drawing
