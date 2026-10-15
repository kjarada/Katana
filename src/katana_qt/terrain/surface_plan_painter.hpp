#pragma once

// The drawing's surfaces in the plan view, each in its display style
// (docs/surface_display.md, "In plan"): the plan painter's surface layer,
// drawn beneath the entities by paintPlan (plan_painter.hpp), so the plan
// view, File > Plot to PDF and every sheet viewport show a surface alike.
//
// WHAT is drawn - the faces' image, which edges, which contours, the labels,
// to what level of detail - is cad::surfacePlan's decision, made and tested
// in katana_cad. This file only draws it through the plan painter's frame:
// the pens (a hairline on screen, ISO pen widths on paper, white printing
// black), the image laid where the model puts it, and the labels turned
// upright.

#include <cstddef>
#include <span>
#include <string>
#include <vector>

#include <QColor>
#include <QFont>
#include <QPointF>
#include <QRectF>
#include <QSizeF>
#include <QString>

#include "katana/cad/plot.hpp"
#include "katana/cad/terrain/surface_plan.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/surface_store.hpp"

class QPainter;

namespace katana::qt {

// Pens: on screen in pixels, on paper in millimetres from the ISO 128 pen
// series (0.13, 0.18, 0.25, 0.35, 0.5 mm) - the finest for the triangles,
// which are context, heavier for what a reader follows: the boundary and the
// major contours.
inline constexpr double kSurfaceTrianglePaperMillimetres = 0.13;
inline constexpr double kSurfaceMinorContourPaperMillimetres = 0.18;
inline constexpr double kSurfaceBreaklinePaperMillimetres = 0.25;
inline constexpr double kSurfaceMajorContourPaperMillimetres = 0.35;
inline constexpr double kSurfaceBoundaryPaperMillimetres = 0.35;
inline constexpr double kSurfaceHeavyLinePixels = 1.5; // boundary, breaklines, majors on screen
// A vertex's mark: a square this wide.
inline constexpr double kSurfacePointPixels = 3.0;
inline constexpr double kSurfacePointPaperMillimetres = 0.5;
// A contour label's text height: ISO 3098's smallest lettering on paper,
// 1.8 mm, and about that at a screen's 96 dpi (1.8 mm is 6.8 px; 10 px reads
// at arm's length).
inline constexpr double kSurfaceLabelPixels = 10.0;
inline constexpr double kSurfaceLabelPaperMillimetres = 1.8;

struct SurfacePaintOptions {
    bool paper = false;
    // The device's pixels per millimetre (PlanPaintOptions::pixelsPerMillimetre).
    double pixelsPerMillimetre = 96.0 / 25.4;
    // On screen, device pixels per view pixel (the display's ratio), so the
    // faces' image has a pixel per device pixel.
    double deviceRatio = 1.0;
    // On paper, the share of the device's resolution the image is embedded
    // at (PlanPainter::imageryShare: the imagery dpi cap), and at most this
    // many pixels.
    double imageryShare = 1.0;
    std::size_t imagePixelCap = 16'000'000;
    // On paper, the plot's colour rule (cad::paperColour, plotImage).
    const katana::cad::PlotSettings* plot = nullptr;
    // The frame's turn (PlanFrame::rotation): labels are kept upright on the
    // device.
    double rotation = 0.0;
    QString fontFamily = QStringLiteral("Segoe UI");
    // What a label is haloed in: the view's ground on screen, the paper on paper.
    QColor halo = QColor(0x1e, 0x23, 0x29);
};

// A legend the surfaces drawn would show: whose, and what it says. The 3D
// scene's are the same card (cad::SceneLayers::legends).
using PaintedLegend = katana::cad::SurfaceLegendCard;

struct SurfacePaintStats {
    std::size_t surfacesDrawn = 0; // visible, with something in view
    std::size_t trianglesInView = 0;
    std::size_t edgesStroked = 0;
    std::size_t trianglesTinted = 0;
    std::size_t contourRuns = 0;
    std::size_t labels = 0;
    std::size_t imagePixels = 0;
    std::size_t pixelsCovered = 0;
    // Why part of a style was not drawn, one for each surface that has one
    // (its name, its style and the reason); empty when all was.
    std::vector<katana::cad::SurfaceProblem> problems;
    // The legends of the surfaces drawn whose styles colour their faces, in
    // the order drawn.
    std::vector<PaintedLegend> legends;
};

// Every visible surface of `surfaces`, in the order they were added, each in
// its style (cad::styleOf), seen through `view` (model to the viewport's
// pixels, y down) on a painter that already carries the frame's origin and
// turn. `visible` is the model box the frame shows (plan_painter.hpp's
// visibleBox). The painter's state is the same afterwards.
SurfacePaintStats paintSurfacesInPlan(QPainter& painter, const katana::terrain::SurfaceStore& surfaces,
                                      const katana::cad::SurfaceStyleSet& styles,
                                      const katana::cad::ViewTransform& view,
                                      const katana::geometry::Box2& visible,
                                      const SurfacePaintOptions& options,
                                      katana::cad::SurfacePlanCache& cache);

// One surface's plan, drawn: what paintSurfacesInPlan draws for each, and
// what the style manager's preview draws.
void paintSurfacePlan(QPainter& painter, const katana::cad::SurfacePlan& plan,
                      const katana::cad::ViewTransform& view, const SurfacePaintOptions& options);

// A legend in a panel whose top-right corner is `topRight`, in the painter's
// units: the caption (the surface and its style), the title, then a swatch a
// band, stop or sector with its label - or, for a smooth ramp or smooth
// stops, one bar from the lowest value at the foot to the highest at the
// head, each stop at its own value along it and the ends always labelled (an
// inner label where it clears the one above it by a line). At most
// kLegendRows rows; past that the last says how many more. Returns the
// panel's rectangle.
inline constexpr int kLegendRows = 16;
// The size that panel will be, without drawing it: what a caller stacking
// legends asks to see whether the next fits. Empty for a legend with no
// entries.
[[nodiscard]] QSizeF measureSurfaceLegend(const katana::cad::SurfaceLegend& legend,
                                          const QString& caption, const QFont& font);
QRectF paintSurfaceLegend(QPainter& painter, const katana::cad::SurfaceLegend& legend,
                          const QString& caption, const QPointF& topRight, const QFont& font,
                          const QColor& text, const QColor& backing);

// What a stack of legends drew: each panel's caption and rectangle, in the
// order drawn.
struct DrawnLegend {
    QString caption;
    QRectF panel;
};

// Where and how a stack of legends goes down a view's corner.
struct LegendStack {
    // The top-right corner of the first panel, in the painter's units.
    QPointF topRight;
    // A later panel whose foot would pass this is left out and ends the stack:
    // a key with its lowest rows cut off by the view's edge reads as a key that
    // ends there. The first is drawn whatever its height (a view too short for
    // it clips it as it clips anything).
    double bottom = 0.0;
    // The gap between one panel's foot and the next one's top.
    double gap = 10.0;
    // At most this many cards are considered (more would cover the drawing
    // they explain); one that is empty counts against it and is not drawn.
    std::size_t maximum = 3;
    QFont font;
    QColor text;
    QColor backing;
};

// The legends of `cards` stacked down from `stack.topRight`, each captioned
// "surface - style", in order: the one stack the plan view and the 3D view
// draw their legends through, so that they stack, fit and leave out alike.
// Each is measured before it is drawn (measureSurfaceLegend), so none is cut
// by the edge and none overlaps another. Returns what was drawn.
std::vector<DrawnLegend> paintSurfaceLegendStack(QPainter& painter,
                                                 std::span<const katana::cad::SurfaceLegendCard> cards,
                                                 const LegendStack& stack);

} // namespace katana::qt
