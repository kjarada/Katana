#pragma once

// A plan's coordinate grid, worked out on paper (docs/plotting.md, "The
// coordinate grid and the live key plan").
//
// A plan viewport shows the ground at a scale, centred on a world point and
// turned by its rotation (sheet_set.hpp). Its grid is the ground's: eastings
// and northings every so many metres, so on a rotated plan it runs askew
// across the paper. It is drawn in one of three styles (GridStyle) and
// labelled where its lines meet the viewport's edges - "E 305 200",
// "N 6 250 400" - each label running along its edge.
//
// Everything here is geometry in paper millimetres (frame.hpp: from the
// paper's bottom-left corner, Y up): which lines, crosses and ticks, and
// where each label goes. The painter strokes the segments and sets the
// labels it is given, so the grid is tested without pixels and plots the
// same from every caller.

#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {
class Document;
}

namespace katana::cad::plotting {

// "none", "ticks", "crosses", "lines": the names the JSON and a command line use.
[[nodiscard]] std::string_view toString(GridStyle style);
[[nodiscard]] std::optional<GridStyle> gridStyleFrom(std::string_view name);

// ---- a plan viewport's ground ----------------------------------------------------

// Where a plan viewport's ground is once "auto" is decided: its scale, and the
// world point at its rectangle's centre. The painter decides an automatic
// viewport's (resolvePlanViewport in the Qt layer); a fixed one's are its own.
struct PlanPlacement {
    double scale = 500.0;
    Point2 centre{};

    friend bool operator==(const PlanPlacement&, const PlanPlacement&) = default;
};

// The viewport's own scale and centre, as stored.
[[nodiscard]] PlanPlacement storedPlacement(const Viewport& viewport);

// Paper to world and back for a plan viewport placed at `at`: the rectangle's
// centre is at.centre, a paper millimetre is at.scale / 1000 metres, and the
// paper's +X runs along the world direction viewport.rotation.
[[nodiscard]] Point2 planPaperToWorld(const Viewport& viewport, const PlanPlacement& at,
                                      const Point2& paper);
[[nodiscard]] Point2 planWorldToPaper(const Viewport& viewport, const PlanPlacement& at,
                                      const Point2& world);

// The ground under the viewport's rectangle: the world points under its four
// corners, counter-clockwise from the one under its bottom-left. Empty for an
// unplaced viewport or a scale that is not positive and finite.
[[nodiscard]] std::vector<Point2> planFootprint(const Viewport& viewport, const PlanPlacement& at);

// ---- the grid ------------------------------------------------------------------------

// What an automatic grid (Viewport::gridInterval 0) is spaced at on the paper,
// roughly: close enough to find a coordinate by eye, far enough apart to leave
// the drawing readable.
inline constexpr double kGridTargetSpacingMm = 50.0;
// The closest a grid may be drawn: a finer one is refused rather than painted
// as a grey smear (and, at a tiny interval, as millions of lines).
inline constexpr double kGridMinimumSpacingMm = 2.0;

// The spacing of an automatic grid at 1:`scale`: of 1, 2 and 5 times a power
// of ten metres, the one whose lines fall nearest kGridTargetSpacingMm apart
// on the paper (nearest by ratio) - 20 m at 1:500, 50 m at 1:1000, 100 m at
// 1:2000. 0 for a scale that is not positive and finite.
[[nodiscard]] double automaticGridInterval(double scale);

// How many decimals a coordinate on a grid of `interval` metres needs: none
// for whole metres, 1 for 0.5 or 2.5, 2 for 0.25 or 0.05, at most 3.
[[nodiscard]] int gridDecimals(double interval);

// A coordinate as a grid label writes it: thousands grouped by a space, with
// `decimals` places - "305 200", "6 250 400.5", "-1 200". A value that rounds
// to zero prints "0", never "-0".
[[nodiscard]] std::string groupedCoordinate(double value, int decimals);

// A line of constant easting runs north-south; one of constant northing runs
// east-west.
enum class GridAxis { Easting, Northing };
// The side of a viewport a line leaves by, and a label stands on.
enum class ViewportEdge { Bottom, Right, Top, Left };

// A grid line across the viewport, on the paper: both ends on its border.
struct GridLine {
    GridAxis axis = GridAxis::Easting;
    double value = 0.0; // its easting or northing, metres
    Point2 from{};
    Point2 to{};
    ViewportEdge fromEdge = ViewportEdge::Bottom;
    ViewportEdge toEdge = ViewportEdge::Top;

    friend bool operator==(const GridLine&, const GridLine&) = default;
};

// A straight stroke on the paper.
struct GridSegment {
    Point2 from{};
    Point2 to{};

    friend bool operator==(const GridSegment&, const GridSegment&) = default;
};

// A label where a grid line meets an edge, as a frame text is set (frame.hpp):
// an anchor, a justification and an angle - 0 along the bottom and top, 90
// (reading up) along the sides - at PlanGridOptions::labelCapMm.
struct GridLabel {
    GridAxis axis = GridAxis::Easting;
    double value = 0.0;
    std::string text; // "E 305 200", "N 6 250 400"
    ViewportEdge edge = ViewportEdge::Bottom;
    Point2 anchor{};
    double angleDegrees = 0.0;
    HorizontalJustify horizontal = HorizontalJustify::Centre;
    VerticalJustify vertical = VerticalJustify::Bottom;
    // The paper the label covers with its white knock-out: what it keeps off
    // the other labels, the viewport's furniture and the edges.
    Box2 box;

    friend bool operator==(const GridLabel&, const GridLabel&) = default;
};

struct PlanGridOptions {
    double labelCapMm = 1.8;   // the labels' cap height
    double tickMm = 2.5;       // Ticks: how far each tick comes in from the border
    double crossMm = 3.0;      // Crosses: the width of each cross
    double labelGapMm = 0.8;   // from the border, or a tick's end, to a label
    double knockOutMm = 0.4;   // the white around a label
    // Paper a label must keep off: the view's title, its scale bar and its
    // north arrow (the painter passes the boxes they knock out).
    std::vector<Box2> keepOut;
    // How wide a label is set, in paper millimetres. Empty: estimated at
    // 0.75 x the cap height a character, which is Arial's figures.
    std::function<double(std::string_view)> labelWidthMm;
    // The least paper between two labels' knock-outs: two labels closer than
    // this read as one ("E 305 240 N 6 250 480"), so the later is left out.
    double labelSpacingMm = 1.5;
};

// What the painter draws for a plan's grid.
struct PlanGrid {
    GridStyle style = GridStyle::None;
    double interval = 0.0; // the spacing drawn, metres
    int decimals = 0;      // in the labels
    // Every grid line crossing the viewport, whatever the style: eastings west
    // to east, then northings south to north.
    std::vector<GridLine> lines;
    // What is stroked: Lines the lines themselves; Crosses the two arms of
    // each cross, along the grid; Ticks one tick in from each end of each line.
    std::vector<GridSegment> strokes;
    // Crosses: the intersections marked, each a whole cross inside the viewport.
    std::vector<Point2> crossings;
    // The labels that fit: inside the viewport, clear of each other and of
    // PlanGridOptions::keepOut, taken edge by edge - bottom, left, top, right -
    // and along each edge in order, so the same grid always keeps the same ones.
    std::vector<GridLabel> labels;

    friend bool operator==(const PlanGrid&, const PlanGrid&) = default;
};

// The grid of plan viewport `viewport` placed at `at`, in its own style and at
// its own interval (automaticGridInterval when that is 0). GridStyle::None
// gives an empty grid. InvalidArgument for an unplaced viewport, a scale that
// is not positive and finite, an interval that is negative or not finite, and
// an interval whose lines would be closer than kGridMinimumSpacingMm on the
// paper.
[[nodiscard]] core::Result<PlanGrid> planGrid(const Viewport& viewport, const PlanPlacement& at,
                                              const PlanGridOptions& options = {});

// Sets the grid of the viewport with id `viewportId` - its style and its
// interval in metres, 0 for automatic - as ONE undoable step. NotFound for no
// such viewport; InvalidArgument for a viewport that is not a plan or a key
// plan, and for an interval that is negative or not finite. Setting what it
// already has records no step.
[[nodiscard]] core::Status setPlanGrid(Document& document, std::string_view viewportId,
                                       GridStyle style, double intervalM = 0.0);

} // namespace katana::cad::plotting
