#pragma once

// The legend on paper (docs/plotting.md, "The smart legend"): what a Legend
// viewport lists as the painter sees it, and each entry's sample drawn as
// the plan prints it.
//
// The legend panel (SheetPainter::paintLegend) gathers the entries here, lays
// them out (cad::plotting::layoutLegend) and sets their labels; the samples
// come from the same resolutions the plan painter uses, so a legend cannot
// show a reader a mark the plan does not print:
//
//   Line    a library linestyle's own strokes (cad::resolveLinePattern and
//           the shared style painter), else a model linetype's dashes, else
//           a plain line - in the entry's colour through the paper colour
//           rule and its weight in paper millimetres; a style's symbol at
//           each end, as the plan puts one at every vertex.
//   Symbol  the symbol at its plotted size: a size on the ground at the
//           legend's scale, the definition's own, or the plain mark's for a
//           built-in shape - shrunk only when it would not fit its cell.
//   Point   the plain point mark, 2 mm across as on the plan.
//   Area    a swatch filled or hatched with the entry's pattern at the
//           legend's scale, outlined as a line of the entry is.
//   Text    "Abc" in the entry's colour.
//
// Reads no widget; the painter's state is the same afterwards.

#include <cstddef>

#include <QRectF>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/style_library.hpp"
#include "sheet_painter.hpp"

class QPainter;

namespace katana::qt {

// What the Legend viewport `legend` on sheet `sheetIndex` of `set` lists:
// cad::plotting::computeLegend at the viewport's scope, each automatic plan
// at the window the painter draws it at (resolvePlanViewport), with the
// source's spatial index and, through its document, the survey code
// library. What the painter prints and what the editor reports of it.
[[nodiscard]] katana::core::Result<katana::cad::plotting::Legend>
gatherLegend(const katana::cad::plotting::SheetSet& set, std::size_t sheetIndex,
             const katana::cad::plotting::Viewport& legend, const SheetSource& source);

struct LegendSampleContext {
    // The drawing's linetypes and hatch patterns; null draws plain lines.
    const katana::entity::Model* model = nullptr;
    // Library linestyles and symbols; null draws the built-in shapes.
    const katana::entity::StyleLibrary* library = nullptr;
    // The session's own colour names, for a pen inside a library definition
    // (StylePaintTarget::colours); null resolves the standard names alone.
    const katana::entity::ColourTable* colours = nullptr;
    // The paper colour rule; null leaves colours as they are.
    const katana::cad::PlotSettings* plot = nullptr;
    // Device pixels per paper millimetre: what a line weight is multiplied by.
    double pixelsPerMillimetre = 300.0 / 25.4;
    // 1 : scale, what a size on the ground is drawn at: a symbol's size in
    // metres, a world linestyle's period, a hatch's spacing. Not positive:
    // 1 : 500.
    double scale = 500.0;
};

// Paints `entry`'s sample into `cell`, in the painter's device units. False
// when nothing was drawn (an empty cell, a symbol that draws nothing).
bool paintLegendSample(QPainter& painter, const katana::cad::plotting::LegendEntry& entry,
                       const QRectF& cell, const LegendSampleContext& context);

} // namespace katana::qt
