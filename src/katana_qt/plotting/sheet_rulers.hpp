#pragma once

// The sheet canvas's rulers and paper grid (docs/plotting.md, "Editing on the
// canvas"): screen furniture, painted over the sheet and never plotted.
//
//   the rulers   along the canvas's top and left edges, in paper millimetres
//                from the paper's bottom-left corner (Y up, as every paper
//                coordinate is), following the zoom and the pan; the paper's
//                span is white on them, the selection's is shaded, and the
//                cursor is marked on both
//   the grid     faint lines every so many millimetres through the same
//                origin: the lines a drag snaps to (plotting::snapToGrid)

#include <optional>

#include <QPointF>
#include <QRectF>
#include <QSize>

#include "katana/geometry/primitives2d.hpp"

class QPainter;

namespace katana::qt {

// How thick the rulers are, in logical pixels.
inline constexpr int kSheetRulerPixels = 20;

// A ruler's numbered step and its tick step, in paper millimetres: the
// smallest of 1, 2, 5, 10, 20 ... mm whose numbers stand at least 50 pixels
// apart, and ticks between them no closer than 5 pixels.
struct RulerSteps {
    double numbered = 10.0;
    double tick = 1.0;
};
[[nodiscard]] RulerSteps rulerSteps(double pixelsPerMillimetre);

// The paper as the canvas shows it, and what the rulers mark on it.
struct RulerView {
    double pixelsPerMillimetre = 1.0;
    // The widget point of the paper's top-left corner.
    QPointF paperOrigin{0.0, 0.0};
    double paperWidthMm = 420.0;
    double paperHeightMm = 297.0;
    // The cursor's paper position, when it is over the canvas.
    std::optional<katana::geometry::Point2> cursor;
    // The selection's extent on the paper; empty for none.
    katana::geometry::Box2 highlight;
};

// The two rulers and the corner between them, over a widget of `size`.
void paintSheetRulers(QPainter& painter, const QSize& size, const RulerView& view);

// Faint lines every `spacingMm` across the paper, every tenth a little
// stronger. Where the lines would crowd closer than 4 pixels only every
// second, fifth or tenth is drawn, so the paper never turns grey.
void paintPaperGrid(QPainter& painter, const RulerView& view, double spacingMm);

} // namespace katana::qt
