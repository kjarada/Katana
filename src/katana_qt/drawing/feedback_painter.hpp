#pragma once

// The one place a tool's preview (cad::ToolFeedback, include/katana/cad/
// tool_feedback.hpp) is drawn over a plan view, and the one home of the
// view's overlay colours. The owner asked on 2026-09-30 for a "visual clue
// where the vertex is going": a preview now says by ROLE what a click takes
// (Target), makes (Added) and takes away (Removed), and each role has its own
// shape as well as its own colour, so it reads without the colour.
//
// The colours are the furniture's - what a view draws over the drawing -
// not the chrome's, so they are here and not in theme.hpp, whose tokens are
// the window's (theme.hpp says the views keep their drawing colours). Each is
// at least 3:1 against the view's ground #1e2329, the WCAG 2.1 (1.4.11) floor
// for graphics that carry meaning: the target green 7.9, the preview cyan
// 8.2, the removed red 5.7, the cold grip blue 4.3.
//
// Nothing here is in the kept drawing image: the view paints this over it
// every frame, so a mouse move costs the marks and never the drawing.

#include <cstddef>
#include <functional>
#include <vector>

#include <QColor>
#include <QPointF>
#include <QRect>
#include <QRectF>
#include <QString>

#include "katana/cad/tool_feedback.hpp"
#include "katana/geometry/primitives2d.hpp"

class QPainter;

namespace katana::qt::drawing {

namespace overlay {
// A tool's ghost and base points, and the prompt's text: the preview cyan
// the plan view has drawn rubber bands in from the start.
[[nodiscard]] QColor preview();
// What a click here takes: the grips' hover green, which already meant that.
[[nodiscard]] QColor target();
// What the step takes away: the chrome's error red (theme::error).
[[nodiscard]] QColor removed();
// The grips (grip_controller.hpp): blue cold, green under the cursor, red hot,
// the colours a drafter already reads.
[[nodiscard]] QColor gripCold();
[[nodiscard]] QColor gripHover();
[[nodiscard]] QColor gripHot();
// The backing of the prompt band, a caption or a label: darker than the
// view, so text over a busy drawing stays legible.
[[nodiscard]] QColor ground();
// Laid under a removed piece before its red dashes: the view's own ground,
// most of the way opaque, so the dashes read over the orange selection.
[[nodiscard]] QColor dim();
// A selection box: a window (left to right) in the preview cyan, a
// crossing (right to left) in green; both see-through.
[[nodiscard]] QColor windowBox();
[[nodiscard]] QColor crossingBox();
} // namespace overlay

// How many of each the last paintFeedback drew, for the view's
// lastPreviewCounts and the headless pointer record.
struct FeedbackCounts {
    std::size_t shapes = 0;
    std::size_t markers = 0;
    std::size_t target = 0;
    std::size_t added = 0;
    std::size_t removed = 0;
    std::size_t focus = 0;
};

// Where a preview is drawn: the view's transform, the plan painter for a
// geometry in the painter's current pen (paintPlanGeometry), the view's
// rectangle, and the cursor the caption sits beside.
struct FeedbackFrame {
    std::function<QPointF(const katana::geometry::Point2&)> toScreen;
    std::function<void(const katana::entity::Geometry&)> drawShape;
    QRectF visible;
    QPointF cursor;
    // A drafting aid's label is drawn below the cursor (the view's
    // trackingLabel_): the caption goes below it.
    bool trackingLabel = false;
    // The prompt band along the bottom, which the caption keeps off.
    double bandHeight = 0.0;
};

// Draws `feedback` in the order that keeps what matters on top: removed
// pieces, target pieces, the ghost, the base points, `focusVertices` (the
// polyline in play, hollow squares), target vertices, removed vertices,
// added vertices, labels, the caption.
FeedbackCounts paintFeedback(QPainter& painter, const katana::cad::ToolFeedback& feedback,
                             const std::vector<katana::geometry::Point2>& focusVertices,
                             const FeedbackFrame& frame);

// The band along the bottom of a view holding a prompt and what has been
// typed for it - a tool's or a picked-up grip's, one look for both.
void paintBand(QPainter& painter, const QRect& view, const QString& text);
// Its height in pixels at the chosen text size.
[[nodiscard]] int bandHeight();

} // namespace katana::qt::drawing
