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
#include <cstdint>
#include <optional>
#include <string_view>

#include "katana/cad/view_transform.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
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

// How a plot's colours print: the plot style's colour mode (docs/plotting.md,
// "Plot styles and output"). Stored by name ("colour", "greyscale",
// "monochrome"), never by number; a new mode goes at the end, and a name is
// never changed.
enum class PlotColourMode {
    Colour,     // as drawn, white printing black (whiteToBlack)
    Greyscale,  // every colour as its luminance: a grey as light as it was
    Monochrome, // every line and letter black; a fill black or white by its lightness
};

[[nodiscard]] std::string_view toString(PlotColourMode mode);
// The name toString gives, or a common other spelling: "color", "grey",
// "gray", "grayscale", "mono", "black". Case is ignored.
[[nodiscard]] std::optional<PlotColourMode> plotColourModeFrom(std::string_view name);

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
    // White and near-white pens print BLACK (decision D7), as AutoCAD's colour
    // 7 does. The screen's ground is dark, so white is what a new layer draws
    // in and what 130 of the 457 map_data rules of a reference mapfile ask
    // for; on white paper every one of those features would vanish. Off only
    // for a plot onto a dark sheet or a deliberate white-on-colour print.
    bool whiteToBlack = true;
    // The plot style. The colour mode is applied by paperColour and
    // paperFillColour, which every paper colour passes through, so a plan, a
    // section and the frame all follow it. The line weight scale multiplies
    // every pen width on paper - a check plot at 0.7, a bold one at 1.4 -
    // and leaves text, dash lengths and symbol sizes as they are.
    PlotColourMode colourMode = PlotColourMode::Colour;
    double lineWeightScale = 1.0;
};

// The lowest a channel may be for a colour to count as white on paper. 230 of
// 255 is within a tenth of white on every channel: a white a screen has been
// dimmed or tinted towards (240, 240, 240) still prints, while "light grey"
// (211, 211, 211) and "light yellow" (255, 255, 224) keep their colour - on
// paper they are faint but they are there, and they are what was asked for.
inline constexpr std::uint8_t kNearWhiteChannel = 230;

// A colour's lightness, 0 (black) to 255 (white): the Rec. 601 luma,
// (299 R + 587 G + 114 B) / 1000, rounded - the weights greyscale printing
// and image tools use for a colour's grey. In integers, so the same colour
// gives the same grey on every machine; a grey is its own luminance.
[[nodiscard]] std::uint8_t luminance(entity::Color colour);

// A monochrome plot's fill is black when its luminance is below this, and
// white paper otherwise: the darker half of the colours print solid and the
// lighter half drop out, as they would on a one-ink plotter, leaving their
// outline (a pen, so black) to show the area.
inline constexpr std::uint8_t kMonochromeFillThreshold = 128;

// The colour a pen prints in. First the white rule: black for white and
// near-white when `settings.whiteToBlack`. Then the colour mode: Colour keeps
// the colour, Greyscale makes it the grey of its luminance, Monochrome makes
// it black. Alpha is kept, so a faded pen stays faded.
[[nodiscard]] entity::Color paperColour(entity::Color colour, const PlotSettings& settings);

// The colour an area fill prints in. Colour and Greyscale print a fill as a
// pen of its colour would (paperColour), so it keeps its lightness as a grey;
// Monochrome prints it black or white by kMonochromeFillThreshold. The white
// of the paper itself is never passed through this: it stays white in every
// mode.
[[nodiscard]] entity::Color paperFillColour(entity::Color colour, const PlotSettings& settings);

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
// then has nothing standard to say. A line along one axis fits on its length:
// a zero dimension asks nothing of the sheet. Fails with InvalidArgument for
// an extent with no size in either direction (empty, or one point) or a
// sheet with no printable area.
[[nodiscard]] core::Result<double> fitScale(const geometry::Box2& extent,
                                            const PlotSettings& settings);

// The finer ladder a SHEET's viewports are scaled on (docs/plotting.md). A
// sheet holds details and sections as well as plans, and a cross section at
// 1 : 100 on a 120 mm cell is 12 m wide - so it needs 1 : 10 to 1 : 75 and the
// in-between 1 : 125, 1 : 150, 1 : 750 and 1 : 1250 that kStandardScales
// leaves out, and 1 : 1 to 1 : 5 for a detail. kStandardScales stays as it is:
// the single-page plot and its tests are built on it.
inline constexpr std::array<double, 24> kSheetScales{
    1.0,    2.0,    5.0,    10.0,   20.0,   25.0,    50.0,    75.0,
    100.0,  125.0,  150.0,  200.0,  250.0,  500.0,   750.0,   1000.0,
    1250.0, 2000.0, 2500.0, 5000.0, 10000.0, 20000.0, 25000.0, 50000.0};

// The first denominator of kSheetScales that is at least `needed` - the
// largest standard scale at which something needing 1 : `needed` still fits -
// or `needed` itself beyond 1 : 50 000, as fitScale does. InvalidArgument for
// a denominator that is not positive and finite.
[[nodiscard]] core::Result<double> sheetScaleAtLeast(double needed);

[[nodiscard]] constexpr double millimetresToPixels(double millimetres, double dpi)
{
    return millimetres * dpi / 25.4; // 25.4 mm to the inch, by definition
}

} // namespace katana::cad
