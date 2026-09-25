#pragma once

// A section fitted to its viewport, and the steps its labels are written at
// (docs/plotting.md, "Section smarts").
//
// A section viewport with autoScale is drawn at the largest standard
// horizontal scale (kSheetScales) at which what it shows fits across its
// plot - a long section's chainage range, or the whole alignment; a cross
// section's two half widths - and at the largest exaggeration of the
// drafter's ladder at which its levels then fit up the plot. The painter
// measures; everything it decides with the numbers is here, so the choice is
// tested without a pixel and an agent can ask what a viewport will be drawn
// at.
//
// Labels decide their own spacing: the grid step along a section is the
// smallest round step (1, 2 or 5 times a power of ten) at which the widest
// label that step writes still clears its neighbours, and the plot is laid
// out around the widest level label, so no value is drawn over another or
// outside its viewport.

#include <array>
#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::plotting {

using katana::geometry::Box2;

// The vertical exaggerations a section is drawn at, smallest first: the
// drafter's ladder. 20 is for the flat long section whose relief would
// otherwise be a line.
inline constexpr std::array<double, 8> kSectionExaggerations{1.0, 2.0,  2.5, 4.0,
                                                             5.0, 8.0, 10.0, 20.0};

// How much of its plot an automatically scaled section fills, each way: the
// rest keeps the ground at the extremes off the plot's frame.
inline constexpr double kSectionFitFill = 0.9;

struct SectionFitRequest {
    // How many metres must fit across the plot: a long section's chainages,
    // a cross section's full width (sectionSpan).
    double spanM = 0.0;
    // How many metres of level must fit up it; 0 when the section is flat or
    // nothing was sampled.
    double depthM = 0.0;
    // The plot the section is drawn in, on paper (sectionPlotLayout).
    double plotWidthMm = 0.0;
    double plotHeightMm = 0.0;
    double fill = kSectionFitFill;
    // The exaggeration when there is no depth to fit: a flat section is not
    // stretched to the ladder's top.
    double flatExaggeration = 1.0;
};

struct SectionFit {
    double scale = 500.0;      // 1 : scale horizontally
    double exaggeration = 1.0; // the vertical scale is scale / exaggeration

    friend bool operator==(const SectionFit&, const SectionFit&) = default;
};

// The scale is the largest standard one at which the span fits the plot's
// width, and at which the depth still fits its height at exaggeration 1 - a
// deep, narrow section is drawn smaller rather than out of its plot. The
// exaggeration is then the largest of kSectionExaggerations at which the
// depth fits the height (fitExaggeration). InvalidArgument for a span that is
// not positive and finite, a plot with no size, a fill outside (0, 1], or a
// negative depth.
[[nodiscard]] core::Result<SectionFit> fitSection(const SectionFitRequest& request);

// The largest of kSectionExaggerations at which `depthM` metres at
// 1 : `scale` fits `heightMm` and the vertical scale, 1 : scale / it, has a
// whole denominator (at 1 : 750, 2.5 - V 1:300 - rather than 4 - V 1:187.5);
// the largest that fits when none of those does; 1 when even that does not
// fit, and `flatExaggeration` when there is no depth.
[[nodiscard]] double fitExaggeration(double depthM, double scale, double heightMm,
                                     double flatExaggeration = 1.0);

// How much of an axis a plot must show: `high - low`, or, around a centre
// the viewport fixes, twice the farther of the two from it. 0 for an empty
// range.
[[nodiscard]] double sectionSpan(double low, double high, std::optional<double> centre = {});

// ---- label steps ---------------------------------------------------------------------

// The first round step (1, 2 or 5 times a power of ten) at least `minimum`;
// 1 for a minimum that is not positive and finite.
[[nodiscard]] double roundStepAtLeast(double minimum);
// The round step after `step`: 1 -> 2 -> 5 -> 10.
[[nodiscard]] double nextRoundStep(double step);

// How many decimals a value on a grid of `step` is written with: none for a
// step of 1 or more, else enough for the step (0.5 -> 1, 0.25 -> 2), at most 3.
[[nodiscard]] int stepDecimals(double step);
// `value` as a label on a grid of `step` ("125", "12.5"); never "-0".
[[nodiscard]] std::string stepText(double value, double step);
// The grid values of `step` in [first, last], at most `cap` of them.
[[nodiscard]] std::vector<double> gridValues(double first, double last, double step,
                                             std::size_t cap = 400);

// The smallest round step at which labels along an axis drawn at
// `mmPerUnit` never overlap: the grid is at least `minimumSpacingMm` apart and
// neighbouring labels, `widestLabelMm(step)` wide at most, leave `gapMm`
// between them. A label written across the axis (a data band's, turned 90
// degrees) passes its height as its width. The widest label is asked of
// each step tried, since a finer step writes more decimals.
[[nodiscard]] double labelStep(double mmPerUnit, double minimumSpacingMm, double gapMm,
                               const std::function<double(double step)>& widestLabelMm);

// ---- the plot's layout ----------------------------------------------------------------

// Paper room around a section's plot, in millimetres.
inline constexpr double kSectionBandRowMm = 8.0;   // a data band row
inline constexpr double kSectionBandHeadMm = 22.0; // the band's row names
inline constexpr double kSectionAxisRowMm = 4.0;   // the axis values under a plot
inline constexpr double kSectionCaptionMm = 4.5;   // a cross section's "CH" caption
inline constexpr double kSectionMinimumBandPlotMm = 30.0; // less: no band

struct SectionLayoutRequest {
    Box2 area;                   // the viewport, less its title's corner
    std::size_t bandRows = 0;    // data band rows wanted (a long section's); 0: none
    bool caption = false;        // a caption under it (a cross section's chainage)
    double levelLabelMm = 9.0;   // the widest level label, drawn left of the plot
};

struct SectionLayout {
    Box2 plot;             // where the section itself is drawn
    bool banded = false;   // the data band fits under it
    double leftMm = 0.0;   // the room left of the plot: level labels or band names
};

// Where a section's plot goes in `area`: the level labels to its left (as
// wide as the widest, and the band's names when there is a band), the band
// or the axis values and the caption below. A band is dropped when it would
// leave the plot less than kSectionMinimumBandPlotMm high; the axis values
// take its place. An empty plot when the area is too small for any.
[[nodiscard]] SectionLayout sectionPlotLayout(const SectionLayoutRequest& request);

// ---- a section viewport ----------------------------------------------------------------

// The strip at a section viewport's foot kept for its title.
inline constexpr double kSectionTitleMm = 7.0;
// A cross section's half width when the viewport gives none.
inline constexpr double kDefaultSectionHalfWidth = 20.0;
// At most so many cross sections are cut for one viewport's interval.
inline constexpr std::size_t kMaximumSectionRows = 64;

// Where a section viewport draws its sections: its rectangle less the
// title's strip.
[[nodiscard]] Box2 sectionDrawingArea(const Box2& rect);
// The chainages a CrossSections viewport cuts at, one row each, top down:
// its stations, else every interval from its first chainage to its last
// (kMaximumSectionRows at most).
[[nodiscard]] std::vector<double> viewportStations(const ViewportSource& source);
// Its sections' half width: its own, else kDefaultSectionHalfWidth.
[[nodiscard]] double viewportHalfWidth(const ViewportSource& source);

} // namespace katana::cad::plotting
