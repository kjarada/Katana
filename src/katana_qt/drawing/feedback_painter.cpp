#include "drawing/feedback_painter.hpp"

#include <algorithm>
#include <cmath>
#include <optional>
#include <variant>

#include <QFontMetrics>
#include <QPainter>
#include <QPainterPath>
#include <QPen>

#include "theme.hpp"

namespace katana::qt::drawing {

namespace overlay {

QColor preview() { return QColor(0x4c, 0xc9, 0xf0); }
QColor target() { return QColor(0x3c, 0xd0, 0x70); }
QColor removed() { return theme::error(); }
QColor gripCold() { return QColor(0x3f, 0x7f, 0xff); }
QColor gripHover() { return target(); }
QColor gripHot() { return QColor(0xf0, 0x40, 0x40); }
QColor ground() { return QColor(0x12, 0x16, 0x1a); }

QColor dim()
{
    QColor colour = theme::viewport();
    colour.setAlpha(175);
    return colour;
}

QColor windowBox()
{
    QColor colour = preview();
    colour.setAlpha(40);
    return colour;
}

QColor crossingBox() { return QColor(0x7b, 0xd3, 0x89, 40); }

QColor snap() { return QColor(0xf7, 0xd0, 0x3c); }

} // namespace overlay

namespace {

using katana::cad::FeedbackMark;
using katana::cad::FeedbackRole;
using katana::entity::PointGeometry;

// Sizes in pixels, whatever the zoom: a mark is furniture, not drawing.
constexpr double kRingRadius = 7.0;
constexpr double kTargetSquare = 6.0;
constexpr double kAddedRadius = 6.0;
constexpr double kPlusArm = 3.0;
// The Removed X and its dark disc sit inside a Target ring's 2 px pen
// (6 to 8 px out): a disc of 7.5 covered the ring's inner half, and a fillet
// - its corner both the target and what goes - read as a Delete.
constexpr double kRemovedHalf = 4.5;
constexpr double kRemovedBacking = 5.5;
constexpr double kFocusSquare = 6.0;
// Focus squares nearer each other than this are thinned: the square and its
// 1.5 px pen are 7.5 px across, so 10 px leaves a gap a squint still sees.
constexpr double kFocusSpacing = 10.0;
constexpr double kMarkerHalf = 3.5;
// The caption's corner is this far right of and below the cursor, clear of
// the crosshair and the snap marker's name; 16 more below a tracking label
// (drawn at +12, +18 by the view).
constexpr double kCaptionOffset = 16.0;
constexpr double kCaptionBelowTracking = 32.0;

bool isVertex(const FeedbackMark& mark)
{
    return std::holds_alternative<PointGeometry>(mark.geometry);
}

QPointF vertexOf(const FeedbackMark& mark, const FeedbackFrame& frame)
{
    return frame.toScreen(std::get<PointGeometry>(mark.geometry).position);
}

// Where a piece's label goes: its middle, from the plan's own geometry.
std::optional<QPointF> middleOf(const katana::entity::Geometry& geometry,
                                const FeedbackFrame& frame)
{
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&geometry)) {
        return frame.toScreen(segment->midpoint());
    }
    if (const auto* arc = std::get_if<katana::geometry::Arc2>(&geometry)) {
        return frame.toScreen(arc->midpoint());
    }
    return std::nullopt;
}

// Text on a small dark chip, so a label reads over whatever it lands on.
void paintLabel(QPainter& painter, const QPointF& corner, const QString& text, const QColor& colour)
{
    const QFont font = theme::overlayFont(10);
    const QFontMetrics metrics(font);
    const QRectF box(corner.x(), corner.y() - metrics.ascent(),
                     metrics.horizontalAdvance(text) + 4.0, metrics.height());
    QColor backing = overlay::ground();
    backing.setAlpha(200);
    painter.setPen(Qt::NoPen);
    painter.setBrush(backing);
    painter.drawRoundedRect(box.adjusted(-1.0, 0.0, 0.0, 0.0), 2.0, 2.0);
    painter.setBrush(Qt::NoBrush);
    painter.setFont(font);
    painter.setPen(colour);
    painter.drawText(QPointF(corner.x() + 1.0, corner.y()), text);
}

void paintCaption(QPainter& painter, const katana::cad::ToolFeedback& feedback,
                  const FeedbackFrame& frame)
{
    const QString text = QString::fromStdString(feedback.caption);
    const QFont font = theme::overlayFont(11);
    const QFontMetrics metrics(font);
    const double padX = 4.0;
    const double padY = 2.0;
    const double stripe = 2.0;
    const double width = metrics.horizontalAdvance(text) + 2 * padX + stripe;
    const double height = metrics.height() + 2 * padY;
    const double below = frame.trackingLabel ? kCaptionBelowTracking : kCaptionOffset;
    double x = frame.cursor.x() + kCaptionOffset;
    double y = frame.cursor.y() + below;
    // Flipped to the other side of the cursor where it would leave the view
    // or run under the prompt band.
    if (x + width > frame.visible.right()) {
        x = frame.cursor.x() - kCaptionOffset - width;
    }
    if (y + height > frame.visible.bottom() - frame.bandHeight) {
        y = frame.cursor.y() - kCaptionOffset - height;
    }
    x = std::max(x, frame.visible.left());
    y = std::max(y, frame.visible.top());
    const QRectF chip(x, y, width, height);
    QColor backing = overlay::ground();
    backing.setAlpha(225);
    painter.setPen(Qt::NoPen);
    painter.setBrush(backing);
    painter.drawRoundedRect(chip, 3.0, 3.0);
    painter.setBrush(feedback.refused ? overlay::removed() : overlay::preview());
    painter.drawRect(QRectF(chip.left(), chip.top(), stripe, chip.height()));
    painter.setBrush(Qt::NoBrush);
    painter.setFont(font);
    painter.setPen(feedback.refused ? overlay::removed() : theme::text());
    painter.drawText(chip.adjusted(stripe + padX, 0.0, -padX, 0.0),
                     Qt::AlignVCenter | Qt::AlignLeft, text);
}

} // namespace

FeedbackCounts paintFeedback(QPainter& painter, const katana::cad::ToolFeedback& feedback,
                             const std::vector<katana::geometry::Point2>& focusVertices,
                             const FeedbackFrame& frame)
{
    FeedbackCounts counts;
    painter.save();
    painter.setBrush(Qt::NoBrush);

    // The polyline in play first, under everything the tool marks: its
    // vertices as the grips' cold squares, hollow, vertex 0 numbered (below)
    // so the numbering the prompts use can be read. Drawn over the marks, a
    // survey string's hundreds of squares were a band that buried the target
    // segment and the removed dashes; and a square is left out where it
    // would crowd the last one drawn, keeping each end, so a dense string
    // shows where its vertices are without becoming a solid bar.
    const QRectF reach = frame.visible.adjusted(-kFocusSquare, -kFocusSquare, kFocusSquare,
                                                kFocusSquare);
    // One drawRects for all of them: a survey string has hundreds, and a
    // call each cost more than the rest of the preview.
    std::vector<QRectF> squares;
    squares.reserve(focusVertices.size());
    std::optional<QPointF> lastDrawn;
    for (std::size_t i = 0; i < focusVertices.size(); ++i) {
        const QPointF p = frame.toScreen(focusVertices[i]);
        const bool end = i == 0 || i + 1 == focusVertices.size();
        if (!reach.contains(p) ||
            (!end && lastDrawn &&
             std::hypot(p.x() - lastDrawn->x(), p.y() - lastDrawn->y()) < kFocusSpacing)) {
            continue;
        }
        squares.emplace_back(p.x() - kFocusSquare / 2, p.y() - kFocusSquare / 2, kFocusSquare,
                             kFocusSquare);
        lastDrawn = p;
    }
    painter.setPen(QPen(overlay::gripCold(), 1.5));
    painter.drawRects(squares.data(), static_cast<int>(squares.size()));
    counts.focus = squares.size();

    // Removed pieces: a dim stroke under red dashes, so the dashes read over
    // the drawing's own lines and the orange selection dashes alike.
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Removed || isVertex(mark)) {
            continue;
        }
        painter.setPen(QPen(overlay::dim(), 5.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        frame.drawShape(mark.geometry);
        QPen dashes(overlay::removed(), 1.6, Qt::CustomDashLine, Qt::FlatCap, Qt::RoundJoin);
        // 5 px on, 4 off: a dash pattern is in pen widths.
        dashes.setDashPattern({5.0 / 1.6, 4.0 / 1.6});
        painter.setPen(dashes);
        frame.drawShape(mark.geometry);
    }
    // A pick the tool refuses keeps its target marks, which say what the pick
    // took, but in the refusal's red: green there promised the click the
    // caption says is turned down.
    const QColor targetColour = feedback.refused ? overlay::removed() : overlay::target();
    // Target pieces: a solid 3 px line along the geometry, arcs as arcs.
    QColor targetStroke = targetColour;
    targetStroke.setAlpha(235);
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Target || isVertex(mark)) {
            continue;
        }
        painter.setPen(QPen(targetStroke, 3.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        frame.drawShape(mark.geometry);
    }
    // The ghost: the result as it will be, dashed, as every tool's always was.
    painter.setPen(QPen(overlay::preview(), 1.0, Qt::DashLine));
    for (const katana::entity::Geometry& shape : feedback.shapes) {
        frame.drawShape(shape);
    }
    counts.shapes = feedback.shapes.size();
    // Base points: a small open square, the grip AutoCAD draws at one.
    painter.setPen(QPen(overlay::preview(), 1.5));
    for (const katana::geometry::Point2& marker : feedback.markers) {
        const QPointF p = frame.toScreen(marker);
        painter.drawRect(QRectF(p.x() - kMarkerHalf, p.y() - kMarkerHalf, 2 * kMarkerHalf,
                                2 * kMarkerHalf));
    }
    counts.markers = feedback.markers.size();
    // What the view puts beneath the glyphs: its snap marker.
    if (frame.beneathGlyphs) {
        painter.save();
        frame.beneathGlyphs();
        painter.restore();
    }
    // Target vertices: a ring round a filled square.
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Target || !isVertex(mark)) {
            continue;
        }
        const QPointF p = vertexOf(mark, frame);
        painter.setPen(QPen(targetColour, 2.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawEllipse(p, kRingRadius, kRingRadius);
        painter.setPen(QPen(overlay::ground(), 1.0));
        painter.setBrush(targetColour);
        painter.drawRect(QRectF(p.x() - kTargetSquare / 2, p.y() - kTargetSquare / 2,
                                kTargetSquare, kTargetSquare));
        painter.setBrush(Qt::NoBrush);
    }
    // Removed vertices: a red X on a dark disc.
    QColor backing = overlay::ground();
    backing.setAlpha(190);
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Removed || !isVertex(mark)) {
            continue;
        }
        const QPointF p = vertexOf(mark, frame);
        painter.setPen(Qt::NoPen);
        painter.setBrush(backing);
        painter.drawEllipse(p, kRemovedBacking, kRemovedBacking);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(QPen(overlay::removed(), 2.5, Qt::SolidLine, Qt::RoundCap));
        painter.drawLine(p + QPointF(-kRemovedHalf, -kRemovedHalf),
                         p + QPointF(kRemovedHalf, kRemovedHalf));
        painter.drawLine(p + QPointF(-kRemovedHalf, kRemovedHalf),
                         p + QPointF(kRemovedHalf, -kRemovedHalf));
    }
    // Enter's place: the Added disc's circle, hollow - nothing is there
    // until Enter is pressed - on a dark outline so it reads over the
    // target's green, with "Enter" beside it (the labels, below).
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Enter || !isVertex(mark)) {
            continue;
        }
        const QPointF p = vertexOf(mark, frame);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(QPen(overlay::ground(), 4.0));
        painter.drawEllipse(p, kAddedRadius, kAddedRadius);
        painter.setPen(QPen(overlay::preview(), 2.0));
        painter.drawEllipse(p, kAddedRadius, kAddedRadius);
        ++counts.enter;
    }
    // Added vertices: a filled disc with a "+", on top of everything else -
    // except over a vertex that goes. A fillet or a chamfer a few pixels
    // across puts its new vertices on the corner's X, and three glyphs on one
    // spot say nothing; the X and the caption do, until the view is zoomed in.
    const auto nearRemoved = [&](const QPointF& p) {
        return std::ranges::any_of(feedback.marks, [&](const FeedbackMark& other) {
            if (other.role != FeedbackRole::Removed || !isVertex(other)) {
                return false;
            }
            const QPointF gone = vertexOf(other, frame);
            return std::hypot(gone.x() - p.x(), gone.y() - p.y()) < kAddedRadius + kRemovedHalf;
        });
    };
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Added || !isVertex(mark)) {
            continue;
        }
        const QPointF p = vertexOf(mark, frame);
        if (nearRemoved(p)) {
            continue;
        }
        painter.setPen(QPen(overlay::ground(), 1.5));
        painter.setBrush(overlay::preview());
        painter.drawEllipse(p, kAddedRadius, kAddedRadius);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(QPen(overlay::ground(), 1.5, Qt::SolidLine, Qt::FlatCap));
        painter.drawLine(p + QPointF(-kPlusArm, 0), p + QPointF(kPlusArm, 0));
        painter.drawLine(p + QPointF(0, -kPlusArm), p + QPointF(0, kPlusArm));
        ++counts.added;
    }
    // Added pieces are the ghost's to show, and neither they nor Enter's
    // place are counted here: the Added vertices and Enter's were counted
    // as drawn, above, and the record says what is on screen.
    for (const FeedbackMark& mark : feedback.marks) {
        switch (mark.role) {
        case FeedbackRole::Target:
            ++counts.target;
            break;
        case FeedbackRole::Removed:
            ++counts.removed;
            break;
        case FeedbackRole::Added:
        case FeedbackRole::Enter:
            break;
        }
    }
    // Labels, above the glyphs: vertex 0 of the polyline in play, then each
    // mark's own (a vertex number, "keep", a height).
    if (!focusVertices.empty()) {
        const QPointF first = frame.toScreen(focusVertices.front());
        // Not where a mark of the tool labels that vertex itself: two chips
        // on one vertex read as neither.
        const bool labelled = std::ranges::any_of(feedback.marks, [&](const FeedbackMark& mark) {
            if (mark.label.empty() || !isVertex(mark)) {
                return false;
            }
            const QPointF at = vertexOf(mark, frame);
            return std::hypot(at.x() - first.x(), at.y() - first.y()) < 1.0;
        });
        if (reach.contains(first) && !labelled) {
            paintLabel(painter, first + QPointF(6.0, -6.0), QStringLiteral("0"),
                       overlay::gripCold().lighter(130));
        }
    }
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.label.empty()) {
            continue;
        }
        const QColor colour = mark.role == FeedbackRole::Removed ? overlay::removed()
                              : mark.role == FeedbackRole::Enter ? overlay::preview()
                                                                 : targetColour;
        std::optional<QPointF> at;
        if (isVertex(mark)) {
            at = vertexOf(mark, frame) + QPointF(9.0, -9.0);
        } else if (const auto middle = middleOf(mark.geometry, frame)) {
            at = *middle + QPointF(6.0, -6.0);
        }
        if (at) {
            paintLabel(painter, *at, QString::fromStdString(mark.label), colour);
        }
    }
    if (!feedback.caption.empty()) {
        paintCaption(painter, feedback, frame);
    }
    painter.restore();
    return counts;
}

int bandHeight()
{
    const QFontMetrics metrics(theme::overlayFont(12));
    return metrics.height() + 2 * 4;
}

void paintBand(QPainter& painter, const QRect& view, const QString& text, Qt::TextElideMode elide)
{
    // The prompt in the view as well as in the window's command line: the
    // eye is on the drawing, and a floating view may be far from the window.
    const QFont font = theme::overlayFont(12);
    const QFontMetrics metrics(font);
    const int pad = 4;
    const int height = metrics.height() + 2 * pad;
    const QRect band(view.left(), view.bottom() + 1 - height, view.width(), height);
    QColor fill = overlay::ground();
    fill.setAlpha(220);
    painter.save();
    painter.fillRect(band, fill);
    painter.setFont(font);
    painter.setPen(overlay::preview());
    painter.drawText(band.adjusted(pad + 2, 0, -pad, 0), Qt::AlignVCenter | Qt::AlignLeft,
                     metrics.elidedText(text, elide, band.width() - 2 * pad - 2));
    painter.restore();
}

} // namespace katana::qt::drawing
