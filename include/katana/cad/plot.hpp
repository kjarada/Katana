#pragma once

// Plotting: the sheet, the scale, and what a millimetre of paper is
// (PLAN.MD Phase 22).
//
// This is the phase that makes `Layer::lineWeight` true. It has been
// documented as "millimetres on paper" since Phase 09, validated and
// persisted, and read by nothing that draws, because there was no paper for
// it to be millimetres of. A plot is the same drawing code the viewport uses,
// painted through a sheet transform instead of the screen's, with the pen
// width taken from the line weight - so a line weight finally does exactly
// what it says, and the viewport and the plot cannot disagree about anything
// but the paper.
//
// Everything here is arithmetic on the sheet and is tested as such; the PDF
// writer and the painter live in the Qt layer, which calls this for the
// numbers.
//
// Model units are metres, as everywhere in the product. A plot at 1 : N puts
// one metre on the paper as 1000 / N millimetres.

#include <array>
#include <cstddef>

#include "katana/cad/view_transform.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// ISO 216 A series, portrait, in millimetres (ISO 216:2007, Table 1).
enum class PaperSize { A0, A1, A2, A3, A4 };

struct PaperDimensions {
    double widthMm = 0.0;
    double heightMm = 0.0;
};

[[nodiscard]] PaperDimensions paperDimensions(PaperSize size, bool landscape);

// The scale ladder a survey drawing is plotted at, as denominators. A fitted
// plot picks the first of these the drawing fits at, so a sheet is 1 : 2000
// and not 1 : 1873 - a scale bar is only useful when the scale is one a
// scale rule carries.
inline constexpr std::array<double, 12> kStandardScales{
    100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0, 20000.0, 25000.0, 50000.0};

struct PlotSettings {
    PaperSize paper = PaperSize::A3;
    bool landscape = true;
    double marginMm = 10.0; // all round; the drawing is fitted inside it
    // Device resolution. 300 dpi is what a PDF viewer and a plotter both
    // resolve; higher makes every hatch line and label cost more for no
    // visible gain.
    double dpi = 300.0;
    double scaleDenominator = 1000.0;  // 1 : N
    geometry::Point2 center{0.0, 0.0}; // the model point at the sheet centre
};

// The sheet as a view transform: pixels here are device pixels of the plot
// at `dpi`, so `view.scale` is device pixels per model unit and the width and
// height are the whole paper.
struct Sheet {
    ViewTransform view;
    double pixelsPerMillimetre = 0.0; // what a line weight is multiplied by
    double widthPixels = 0.0;
    double heightPixels = 0.0;
};

// Fails with InvalidArgument for a non-positive resolution or scale, a
// margin that leaves no printable area, or a non-finite centre.
[[nodiscard]] core::Result<Sheet> sheetFor(const PlotSettings& settings);

// The smallest standard denominator at which `extent` fits inside the
// printable area of the sheet, or the exact denominator needed when even
// 1 : 50 000 is too large - a plot always fits; it is only the scale bar that
// then has nothing standard to say. Fails with InvalidArgument for an empty
// extent or a sheet with no printable area.
[[nodiscard]] core::Result<double> fitScale(const geometry::Box2& extent,
                                            const PlotSettings& settings);

[[nodiscard]] constexpr double millimetresToPixels(double millimetres, double dpi)
{
    return millimetres * dpi / 25.4; // 25.4 mm to the inch, by definition
}

} // namespace katana::cad
