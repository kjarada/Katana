#include "drawing/feedback_painter.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <numbers>
#include <optional>
#include <variant>
#include <vector>

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
// The refusal's stroke runs corner to corner inside its ring, the ring's
// radius times cos 45° from the middle each way: the struck ring of "not here".
constexpr double kStruckHalf = kRingRadius * std::numbers::sqrt2 / 2.0;
// Enter's place: the Added disc's circle round a dot, its middle the dark
// ground. Hollow, the segment through it read as a circled minus beside the
// new vertex's circled plus - "add" and "remove" for two places that add.
constexpr double kEnterDot = 1.75;
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
// Where a vertex mark's label and a piece's label sit from their point.
constexpr double kVertexLabelOffset = 9.0;
constexpr double kPieceLabelOffset = 6.0;

bool isVertex(const FeedbackMark& mark)
{
    return std::holds_alternative<PointGeometry>(mark.geometry);
}

QPointF vertexOf(const FeedbackMark& mark, const FeedbackFrame& frame)
{
    return frame.toScreen(std::get<PointGeometry>(mark.geometry).position);
}

// Where a piece's label and its refusal's glyph go: its middle, from the
// plan's own geometry - a whole polyline's, the middle of its middle segment.
std::optional<QPointF> middleOf(const katana::entity::Geometry& geometry,
                                const FeedbackFrame& frame)
{
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&geometry)) {
        return frame.toScreen(segment->midpoint());
    }
    if (const auto* arc = std::get_if<katana::geometry::Arc2>(&geometry)) {
        return frame.toScreen(arc->midpoint());
    }
    if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&geometry);
        polyline != nullptr && polyline->segmentCount() > 0) {
        return frame.toScreen(polyline->segment(polyline->segmentCount() / 2).midpoint());
    }
    if (const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&geometry);
        curve != nullptr && curve->segmentCount() > 0) {
        return frame.toScreen(std::visit([](const auto& piece) { return piece.pointAt(0.5); },
                                         curve->segment(curve->segmentCount() / 2)));
    }
    return std::nullopt;
}

// Whether `mark` is drawn as the refusal: flagged by the tool, or - in a
// refused preview that flags none - a Target, the pick that is turned down
// (ToolFeedback::refused).
bool drawnRefused(const FeedbackMark& mark, const katana::cad::ToolFeedback& feedback,
                  bool anyFlagged)
{
    return mark.refused ||
           (feedback.refused && !anyFlagged && mark.role == FeedbackRole::Target);
}

// The refusal's glyph: a ring struck through, the sign for "not here", on a
// dark disc so it reads over any line. Its SHAPE says refused, not only its
// red: a Target's ring round a filled square, drawn red, was the same khaki
// ring as the accepted green one to a red-green colour-blind eye (Machado,
// Oliveira and Fernandes 2009: the two a CIE76 difference of about 5).
void paintStruck(QPainter& painter, const QPointF& p)
{
    QColor backing = overlay::ground();
    backing.setAlpha(190);
    painter.setPen(Qt::NoPen);
    painter.setBrush(backing);
    painter.drawEllipse(p, kRingRadius, kRingRadius);
    painter.setBrush(Qt::NoBrush);
    painter.setPen(QPen(overlay::removed(), 2.0));
    painter.drawEllipse(p, kRingRadius, kRingRadius);
    painter.setPen(QPen(overlay::removed(), 2.0, Qt::SolidLine, Qt::FlatCap));
    painter.drawLine(p + QPointF(-kStruckHalf, -kStruckHalf), p + QPointF(kStruckHalf, kStruckHalf));
}

// The box a label's chip covers with its baseline's left end at `corner`:
// the one sum paintLabel draws and the caption keeps off.
QRectF labelBox(const QPointF& corner, const QString& text)
{
    const QFontMetrics metrics(theme::overlayFont(10));
    return QRectF(corner.x() - 1.0, corner.y() - metrics.ascent(),
                  metrics.horizontalAdvance(text) + 5.0, metrics.height());
}

// Text on a small dark chip, so a label reads over whatever it lands on.
void paintLabel(QPainter& painter, const QPointF& corner, const QString& text, const QColor& colour)
{
    QColor backing = overlay::ground();
    backing.setAlpha(200);
    painter.setPen(Qt::NoPen);
    painter.setBrush(backing);
    painter.drawRoundedRect(labelBox(corner, text), 2.0, 2.0);
    painter.setBrush(Qt::NoBrush);
    painter.setFont(theme::overlayFont(10));
    painter.setPen(colour);
    painter.drawText(QPointF(corner.x() + 1.0, corner.y()), text);
}

// Where each mark's label goes, when it has one and a place.
std::optional<QPointF> labelCorner(const FeedbackMark& mark, const FeedbackFrame& frame)
{
    if (isVertex(mark)) {
        return vertexOf(mark, frame) + QPointF(kVertexLabelOffset, -kVertexLabelOffset);
    }
    if (const auto middle = middleOf(mark.geometry, frame)) {
        return *middle + QPointF(kPieceLabelOffset, -kPieceLabelOffset);
    }
    return std::nullopt;
}

// What the caption keeps off: every vertex glyph and piece middle a mark
// draws, and every label.
std::vector<QRectF> markBoxes(const katana::cad::ToolFeedback& feedback, const FeedbackFrame& frame)
{
    const double r = kRingRadius + 1.0;
    std::vector<QRectF> boxes;
    for (const FeedbackMark& mark : feedback.marks) {
        const std::optional<QPointF> at =
            isVertex(mark) ? std::optional<QPointF>(vertexOf(mark, frame))
                           : middleOf(mark.geometry, frame);
        if (at) {
            boxes.emplace_back(at->x() - r, at->y() - r, 2 * r, 2 * r);
        }
        if (!mark.label.empty()) {
            if (const auto corner = labelCorner(mark, frame)) {
                boxes.push_back(labelBox(*corner, QString::fromStdString(mark.label)));
            }
        }
    }
    return boxes;
}

// The caption's chip at the first of the cursor's four corners - below and
// right, above and right, below and left, above and left - each moved inside
// the view and off the prompt band, that covers no mark and not the cursor
// itself; where every one covers something, the one over fewest. It sat
// below and right whatever was there, and hid the "Enter" place a user who
// chose a vertex first needs to see.
QRectF captionChip(const QSizeF& size, const FeedbackFrame& frame,
                   const std::vector<QRectF>& keepOff)
{
    const double below = frame.trackingLabel ? kCaptionBelowTracking : kCaptionOffset;
    const QPointF c = frame.cursor;
    const QRectF room = frame.visible.adjusted(0.0, 0.0, 0.0, -frame.bandHeight);
    // The crosshair's middle, which a chip pushed back into a small view
    // could land on.
    const QRectF cursor(c.x() - kCaptionOffset / 2, c.y() - kCaptionOffset / 2, kCaptionOffset,
                        kCaptionOffset);
    const double right = c.x() + kCaptionOffset;
    const double left = c.x() - kCaptionOffset - size.width();
    const double under = c.y() + below;
    const double over = c.y() - kCaptionOffset - size.height();
    QRectF best;
    std::size_t bestCovered = std::numeric_limits<std::size_t>::max();
    for (const QPointF& corner : {QPointF(right, under), QPointF(right, over),
                                  QPointF(left, under), QPointF(left, over)}) {
        QRectF chip(corner, size);
        chip.moveLeft(std::clamp(chip.left(), room.left(),
                                 std::max(room.left(), room.right() - size.width())));
        chip.moveTop(std::clamp(chip.top(), room.top(),
                                std::max(room.top(), room.bottom() - size.height())));
        const auto covered =
            static_cast<std::size_t>(std::ranges::count_if(
                keepOff, [&chip](const QRectF& box) { return chip.intersects(box); })) +
            (chip.intersects(cursor) ? 1U : 0U);
        if (covered < bestCovered) {
            best = chip;
            bestCovered = covered;
        }
        if (covered == 0) {
            break;
        }
    }
    return best;
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
    const QRectF chip = captionChip(QSizeF(width, height), frame, markBoxes(feedback, frame));
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
    // A mark that says why a click is refused keeps its place, which says
    // what the pick took, but in the refusal's red AND its shape - the ring
    // struck through (paintStruck): green there promised the click the
    // caption says is turned down, and red alone is green to many eyes.
    const bool anyFlagged = std::ranges::any_of(
        feedback.marks, [](const FeedbackMark& mark) { return mark.refused; });
    const auto refusedMark = [&](const FeedbackMark& mark) {
        return drawnRefused(mark, feedback, anyFlagged);
    };
    // Target pieces: a solid 3 px line along the geometry, arcs as arcs; a
    // refused one red, with the struck ring at its middle (below).
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Target || isVertex(mark)) {
            continue;
        }
        QColor stroke = refusedMark(mark) ? overlay::removed() : overlay::target();
        stroke.setAlpha(235);
        painter.setPen(QPen(stroke, 3.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
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
        if (mark.role != FeedbackRole::Target || !isVertex(mark) || refusedMark(mark)) {
            continue;
        }
        const QPointF p = vertexOf(mark, frame);
        painter.setPen(QPen(overlay::target(), 2.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawEllipse(p, kRingRadius, kRingRadius);
        painter.setPen(QPen(overlay::ground(), 1.0));
        painter.setBrush(overlay::target());
        painter.drawRect(QRectF(p.x() - kTargetSquare / 2, p.y() - kTargetSquare / 2,
                                kTargetSquare, kTargetSquare));
        painter.setBrush(Qt::NoBrush);
    }
    // Removed vertices: a red X on a dark disc.
    QColor backing = overlay::ground();
    backing.setAlpha(190);
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Removed || !isVertex(mark) || refusedMark(mark)) {
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
    // What Enter acts on: the Added disc's circle round a dot, its middle the
    // dark ground - nothing is there until Enter is pressed - so the line
    // through it stops at the ring and no circled minus sits beside the new
    // vertex's circled plus; on a dark outline so it reads over the
    // target's green, with "Enter" beside it (the labels, below).
    for (const FeedbackMark& mark : feedback.marks) {
        if (mark.role != FeedbackRole::Enter || !isVertex(mark)) {
            continue;
        }
        ++counts.enter;
        if (refusedMark(mark)) {
            continue; // struck, below
        }
        const QPointF p = vertexOf(mark, frame);
        painter.setPen(Qt::NoPen);
        painter.setBrush(overlay::ground()); // opaque: no line shows through
        painter.drawEllipse(p, kAddedRadius, kAddedRadius);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(QPen(overlay::ground(), 4.0));
        painter.drawEllipse(p, kAddedRadius, kAddedRadius);
        painter.setPen(QPen(overlay::preview(), 2.0));
        painter.drawEllipse(p, kAddedRadius, kAddedRadius);
        painter.setPen(Qt::NoPen);
        painter.setBrush(overlay::preview());
        painter.drawEllipse(p, kEnterDot, kEnterDot);
        painter.setBrush(Qt::NoBrush);
    }
    // The refused marks: the struck ring on a vertex, and at a piece's middle.
    for (const FeedbackMark& mark : feedback.marks) {
        if (!refusedMark(mark)) {
            continue;
        }
        if (isVertex(mark)) {
            paintStruck(painter, vertexOf(mark, frame));
        } else if (const auto middle = middleOf(mark.geometry, frame)) {
            paintStruck(painter, *middle);
        }
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
        const QColor colour = refusedMark(mark) || mark.role == FeedbackRole::Removed
                                  ? overlay::removed()
                              : mark.role == FeedbackRole::Enter ? overlay::preview()
                                                                 : overlay::target();
        if (const auto at = labelCorner(mark, frame)) {
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

QString paintBand(QPainter& painter, const QRect& view, const QString& text,
                  Qt::TextElideMode elide)
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
    const QString line = metrics.elidedText(text, elide, band.width() - 2 * pad - 2);
    painter.save();
    painter.fillRect(band, fill);
    painter.setFont(font);
    painter.setPen(overlay::preview());
    painter.drawText(band.adjusted(pad + 2, 0, -pad, 0), Qt::AlignVCenter | Qt::AlignLeft, line);
    painter.restore();
    return line;
}

} // namespace katana::qt::drawing
